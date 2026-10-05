//! パスワードハッシュ化・検証とユーザー認証。ログインID単位のレートリミットも扱う。
//! HTTPハンドラ(login/logout/me)は `api::auth` を参照。

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use argon2::password_hash::PasswordHasher;
use argon2::{Argon2, PasswordVerifier};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tower_sessions::Session;
use utoipa::ToSchema;

use crate::error::AppError;
use crate::state::AppState;

/// ユーザーの権限。管理者向け機能(コンテンツ管理等)の土台。
/// `sqlx::Type` により `users.role` の TEXT カラム(`'admin'`/`'user'`)と相互変換する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to hash password")]
    Hash(#[source] argon2::password_hash::Error),
    #[error("failed to verify password hash")]
    Verify(#[source] argon2::password_hash::Error),
    /// セッションストア(DB)側のエラー。クライアント起因ではないので5xxとして扱う
    /// (`AppError::Internal`経由)。「未ログイン」の401とは明確に区別する。
    #[error("session operation failed")]
    Session(#[from] tower_sessions::session::Error),
    /// `SessionManagerLayer`が掛かっていないルートで`Session`を抽出しようとした場合。
    /// 実際には到達しないはず(常にレイヤーを掛けているため)だが、抽出失敗として
    /// 型で表現しておく。
    #[error("session layer is not configured: {0}")]
    MissingSessionLayer(&'static str),
    /// パスワード検証(`spawn_blocking`)タスクの実行に失敗した場合(panic等)。
    #[error("authentication task failed")]
    Join(#[from] tokio::task::JoinError),
    #[error("failed to generate random bytes")]
    Random(#[source] getrandom::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum CreateUserError {
    /// 大文字小文字だけが違う名前も、同じ名前として扱う (`users_username_nocase`)。
    #[error("username already exists")]
    UsernameTaken(String),
    #[error("database operation failed")]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Hash(#[from] Error),
}

/// 新規作成されたユーザーの、呼び出し元が必要とする最小限の情報。
pub struct CreatedUser {
    pub id: i64,
    pub created_at: String,
}

/// ユーザーを作成する。CLI (`--create-user`) とユーザー管理APIの両方から呼ぶ共通処理。
/// `recovery_code_hash` は、作ると同時にリカバリコードを持たせるとき (セットアップ) だけ渡す。
pub async fn create_user(
    pool: &SqlitePool,
    username: &str,
    password: &str,
    role: Role,
    recovery_code_hash: Option<&str>,
) -> Result<CreatedUser, CreateUserError> {
    let hash = hash_password_blocking(password).await?;

    let result = sqlx::query!(
        r#"INSERT INTO users (username, password_hash, role, recovery_code_hash) VALUES (?, ?, ?, ?)
           RETURNING id as "id!", created_at"#,
        username,
        hash,
        role,
        recovery_code_hash
    )
    .fetch_one(pool)
    .await;

    match result {
        Ok(row) => Ok(CreatedUser {
            id: row.id,
            created_at: row.created_at,
        }),
        Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
            Err(CreateUserError::UsernameTaken(username.to_string()))
        }
        Err(err) => Err(CreateUserError::Db(err)),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RenameUserError {
    #[error("username already exists")]
    UsernameTaken(String),
    #[error("user not found")]
    NotFound,
    #[error("database operation failed")]
    Db(#[from] sqlx::Error),
}

/// ユーザー名を変える。本人の変更とユーザー管理の両方から呼ぶ共通処理。
/// セッションは user id で持つので、変えてもログインは切れない。
pub async fn rename_user(
    executor: impl sqlx::SqliteExecutor<'_>,
    id: i64,
    username: &str,
) -> Result<(), RenameUserError> {
    let result = sqlx::query!("UPDATE users SET username = ? WHERE id = ?", username, id)
        .execute(executor)
        .await;
    match result {
        Ok(done) if done.rows_affected() == 0 => Err(RenameUserError::NotFound),
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
            Err(RenameUserError::UsernameTaken(username.to_string()))
        }
        Err(err) => Err(RenameUserError::Db(err)),
    }
}

pub fn hash_password(password: &str) -> Result<String, Error> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(Error::Hash)
}

/// `hash_password` を `spawn_blocking` に載せて実行する。
pub async fn hash_password_blocking(password: &str) -> Result<String, Error> {
    let password = password.to_string();
    tokio::task::spawn_blocking(move || hash_password(&password)).await?
}

/// パスワードを検証する。`Ok(false)` は「単に不一致」、`Err` はDB内のハッシュが壊れている
/// 等の想定外の失敗で、呼び出し側で区別できるようにしている
/// (両方を`false`にまとめると、壊れたハッシュが「パスワード不一致」として握り潰されてしまう)。
pub fn verify_password(password: &str, hash: &str) -> Result<bool, Error> {
    match Argon2::default().verify_password(password.as_bytes(), hash) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::PasswordInvalid) => Ok(false),
        Err(err) => Err(Error::Verify(err)),
    }
}

/// リカバリコードの字 (Crockford の base32)。紛らわしい `I`・`L`・`O`・`U` を含まない。
const RECOVERY_CODE_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const RECOVERY_CODE_LEN: usize = 20;
const RECOVERY_CODE_GROUP: usize = 5;

/// 新しいリカバリコードを作る (→ docs/access.md「リカバリコード」)。見せる形 (5文字ずつ `-` で区切る) で返す。
pub fn generate_recovery_code() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; RECOVERY_CODE_LEN];
    getrandom::fill(&mut bytes)?;
    // 256 は 32 で割り切れるので、下位5ビットを取っても偏らない。
    let chars: Vec<char> = bytes
        .iter()
        .map(|b| RECOVERY_CODE_ALPHABET[usize::from(b & 0x1f)] as char)
        .collect();
    Ok(chars
        .chunks(RECOVERY_CODE_GROUP)
        .map(|group| group.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-"))
}

/// 入力されたリカバリコードを、ハッシュを取る形に揃える。区切り・空白・大文字小文字・
/// 紛らわしい字 (`O`→`0`、`I`/`L`→`1`) の違いを許す。コードになりえない入力は `None`。
pub fn normalize_recovery_code(input: &str) -> Option<String> {
    let normalized: String = input
        .chars()
        .filter(|c| *c != '-' && !c.is_whitespace())
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        })
        .collect();
    let valid = normalized.len() == RECOVERY_CODE_LEN
        && normalized
            .bytes()
            .all(|b| RECOVERY_CODE_ALPHABET.contains(&b));
    valid.then_some(normalized)
}

/// 新しいリカバリコードを作り、見せる形とハッシュを返す。ハッシュはパスワードと同じ Argon2。
pub async fn new_recovery_code() -> Result<(String, String), Error> {
    let code = generate_recovery_code().map_err(Error::Random)?;
    let normalized = normalize_recovery_code(&code).expect("generated code is valid");
    let hash = hash_password_blocking(&normalized).await?;
    Ok((code, hash))
}

/// `username` の利用者のリカバリコードを照合し、合えばその利用者の id と照合に使ったハッシュを返す。
/// ユーザーがいない・コードが無いときも、ダミーのハッシュで照合してから
/// `None` を返す (ログインと同じく、処理時間で有無を探らせないため)。
pub async fn verify_recovery_code(
    pool: &SqlitePool,
    username: &str,
    code: &str,
) -> Result<Option<(i64, String)>, AppError> {
    let row = sqlx::query!(
        r#"SELECT id as "id!", recovery_code_hash FROM users
           WHERE username = ? COLLATE NOCASE"#,
        username
    )
    .fetch_optional(pool)
    .await?;
    // 形が合わない入力も、空文字として同じ照合を通す (必ず合わない)。
    let normalized = normalize_recovery_code(code).unwrap_or_default();
    let stored = row.and_then(|row| row.recovery_code_hash.map(|hash| (row.id, hash)));
    tokio::task::spawn_blocking(move || match stored {
        Some((id, hash)) => {
            let ok = verify_password(&normalized, &hash)?;
            Ok::<_, Error>(ok.then_some((id, hash)))
        }
        None => {
            verify_password(&normalized, &DUMMY_HASH)?;
            Ok(None)
        }
    })
    .await
    .map_err(Error::Join)?
    .map_err(AppError::from)
}

/// ユーザーが存在しない場合でも、存在する場合とほぼ同じ処理時間になるように
/// 検証だけは必ず行う(タイミングでユーザー名の有無を推測されないようにするため)。
/// 起動のたびに(実際に使われるArgon2のパラメータで)生成するため、パラメータの既定値が
/// 変わっても実ハッシュと同コストであることが保証される。このハッシュ自体でログインが
/// 成立することはない(対応する平文パスワードを知らないため)。
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| {
    hash_password("dummy-password-for-timing-safety").expect("failed to hash dummy password")
});

pub const SESSION_USER_ID_KEY: &str = "user_id";

struct UserRow {
    id: i64,
    username: String,
    password_hash: String,
    role: Role,
    has_recovery_code: bool,
}

/// ユーザー名・パスワードを検証する。ユーザーが存在しない場合は `DUMMY_HASH` と照合してから
/// `None` を返す(タイミング攻撃対策)。
///
/// Argon2の検証はCPUバウンドな処理のため `spawn_blocking` で非同期ランタイムをブロック
/// しないようにする。
pub async fn authenticate(
    pool: &SqlitePool,
    username: &str,
    password: &str,
) -> Result<Option<AuthUser>, AppError> {
    // username は大文字小文字を無視して照合する(users_username_nocase インデックス側と
    // 意味を合わせるため、比較にも明示的に COLLATE NOCASE を指定する)。
    let row = sqlx::query_as!(
        UserRow,
        r#"SELECT id as "id!", username, password_hash, role as "role: Role",
                  recovery_code_hash IS NOT NULL as "has_recovery_code!: bool"
           FROM users WHERE username = ? COLLATE NOCASE"#,
        username
    )
    .fetch_optional(pool)
    .await?;

    let password = password.to_string();
    tokio::task::spawn_blocking(move || verify_row(row, &password))
        .await
        .map_err(Error::Join)?
        .map_err(AppError::from)
}

fn verify_row(row: Option<UserRow>, password: &str) -> Result<Option<AuthUser>, Error> {
    match row {
        Some(row) => {
            let ok = verify_password(password, &row.password_hash)?;
            Ok(ok.then_some(AuthUser {
                id: row.id,
                username: row.username,
                role: row.role,
                has_recovery_code: row.has_recovery_code,
            }))
        }
        None => {
            // ユーザーが存在しない場合もダミーハッシュに対して検証だけは走らせる(必ずfalseになる)。
            verify_password(password, &DUMMY_HASH)?;
            Ok(None)
        }
    }
}

/// レートリミッタが Map のキーとして追跡するユーザー名長の上限(バイト数)。
/// 制限が無いと、巨大なユーザー名を大量に送るだけでメモリを圧迫できてしまう。この長さで
/// 打ち切って集計するため、冒頭がこの長さと一致する別々のユーザー名は同じ試行回数として
/// 扱われるが、レートリミット用途では許容できるトレードオフとする。
const MAX_TRACKED_USERNAME_LEN: usize = 256;

/// UTF-8の文字境界を壊さずに `MAX_TRACKED_USERNAME_LEN` 以内に切り詰める。
fn truncate_username(username: &str) -> &str {
    if username.len() <= MAX_TRACKED_USERNAME_LEN {
        return username;
    }
    let mut end = MAX_TRACKED_USERNAME_LEN;
    while !username.is_char_boundary(end) {
        end -= 1;
    }
    &username[..end]
}

/// ユーザー名単位でログイン試行回数を制限する、プロセス内in-memoryなレートリミッタ。
/// Argon2の検証コストはリクエストごとに発生するため、制限が無いとパスワード総当たりだけ
/// でなくCPU枯渇DoSの余地もある。
///
/// ユーザー名単位の制限だけでは、攻撃者が毎回異なるユーザー名を送ることで際限なく
/// Argon2を実行させられる上、Mapのキーも無制限に増え続けてしまう。これを防ぐため、
/// 以下の3つを組み合わせている。
///
/// - 全リクエスト共通のグローバル上限も別途課す
/// - 追跡する distinct ユーザー名数に上限を設け、超過時は新規ユーザー名を拒否する
/// - 呼び出しのたびに期限切れの試行を掃除し、空になったキーはMapから削除する
///
/// 単一バイナリで自己ホストする小規模用途を想定した簡易実装であり、プロセス再起動や
/// マルチインスタンス運用をまたいだ制限は行わない。IPアドレス単位の制限ではないため、
/// 同一IPからの分散的な試行を個別には識別できない点に注意(グローバル上限が全体としては効く)。
///
/// トレードオフとして、存在しないユーザー名を連続送信するだけで第三者がグローバル上限に
/// 到達させられ、その間は正当なユーザーも含めて全員がログインできなくなる(可用性DoS)。
/// IPアドレス単位の制限にはリバースプロキシ配下でのX-Forwarded-For解析等の追加設計が要り、
/// LAN内自己ホスト・少人数利用を前提とする現状ではスコープ外としている。公開ネットワークからの
/// 到達性を前提にする場合は要再検討。
pub struct LoginRateLimiter {
    max_attempts: u32,
    global_max_attempts: u32,
    max_tracked_ids: usize,
    window: Duration,
    attempts: Mutex<HashMap<Key, Vec<Instant>>>,
}

#[derive(Hash, Eq, PartialEq, Clone)]
enum Key {
    /// ユーザー名に依存しない、全体の試行回数を数えるためのキー。
    Global,
    User(String),
}

/// レートリミット用のユーザーキーを作る。ログイン照合(`COLLATE NOCASE`)と揃えて大文字小文字を
/// 無視するため、ASCII小文字化してから使う(`NOCASE`自体もASCII範囲のみを無視する仕様)。
/// これが無いと `alice`/`Alice`/`ALICE` がそれぞれ別バケットとして扱われ、ユーザー単位の
/// 試行回数制限が大文字小文字を変えるだけで回避できてしまう。
fn rate_limit_key(username: &str) -> Key {
    Key::User(truncate_username(username).to_ascii_lowercase())
}

impl LoginRateLimiter {
    pub fn new(max_attempts: u32, global_max_attempts: u32, window: Duration) -> Self {
        Self {
            max_attempts,
            global_max_attempts,
            max_tracked_ids: 10_000,
            window,
            attempts: Mutex::new(HashMap::new()),
        }
    }

    /// `username` について、直近 `window` 以内の試行回数がユーザー単位・全体単位どちらの
    /// 上限も未満なら、この呼び出し自体を1回の試行として記録し `true` を返す。上限に達して
    /// いる場合は記録せず `false` を返す。
    pub fn try_acquire(&self, username: &str) -> bool {
        let now = Instant::now();
        let user_key = rate_limit_key(username);

        // 別リクエストのロック中にpanicした場合でも制限機能自体は継続させたいので、
        // poisonした場合は中身を引き継いで使う。
        let mut attempts = self.attempts.lock().unwrap_or_else(|e| e.into_inner());

        // 期限切れの試行を掃除し、空になったキーはMapから削除する。これによりdistinctな
        // ユーザー名を送り続けられてもMapが際限なく肥大化しないようにする。
        attempts.retain(|_, v| {
            v.retain(|&t| now.duration_since(t) < self.window);
            !v.is_empty()
        });

        // 全体の試行回数がグローバル上限に達していれば、ユーザー名に関わらず拒否する
        // (異なるユーザー名を使ったCPU枯渇DoS対策)。
        if attempts.entry(Key::Global).or_default().len() as u32 >= self.global_max_attempts {
            return false;
        }

        // 追跡中のdistinctユーザー名数が上限に達しており、かつこのユーザー名が未追跡なら、
        // Mapをこれ以上増やさないため拒否する(直前で必ず作られるKey::Globalの分を1件除いて
        // 数える)。
        let tracked_user_count = attempts.len().saturating_sub(1);
        if !attempts.contains_key(&user_key) && tracked_user_count >= self.max_tracked_ids {
            return false;
        }

        if attempts.entry(user_key.clone()).or_default().len() as u32 >= self.max_attempts {
            return false;
        }

        attempts.entry(Key::Global).or_default().push(now);
        attempts.entry(user_key).or_default().push(now);
        true
    }

    /// ログイン成功時に呼ぶ。直前の `try_acquire` で記録した1回分の試行を、
    /// ユーザー単位・グローバル単位の両方から取り消す。これにより、正しい
    /// パスワードでの連続ログインがレートリミットを消費し続けることはない。
    ///
    /// 同一ユーザー名への並行リクエストがある場合、取り消す試行が入れ替わる可能性が
    /// あるが、影響は「別の正当なリクエストの分を1回免除する」程度に留まり、
    /// レートリミットが安全側(緩む方向)にしかズレないため許容する。
    pub fn release(&self, username: &str) {
        let user_key = rate_limit_key(username);
        let mut attempts = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(v) = attempts.get_mut(&user_key) {
            v.pop();
        }
        if let Some(v) = attempts.get_mut(&Key::Global) {
            v.pop();
        }
    }
}

/// ログイン済みユーザーを表すextractor。ハンドラの引数に `AuthUser` を書くだけで
/// 未ログインなら401 `unauthorized` になる。
pub struct AuthUser {
    pub id: i64,
    pub username: String,
    pub role: Role,
    /// リカバリコードを作ってあるか (→ docs/access.md「リカバリコード」)。
    pub has_recovery_code: bool,
}

impl AuthUser {
    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // `SessionManagerLayer`が掛かっていない場合のみ発生する(常に掛けているので通常は
        // 到達しない)。DB障害ではなくサーバー側の設定ミスなので、こちらもInternal扱いにする。
        let session = Session::from_request_parts(parts, state)
            .await
            .map_err(|(_, msg)| AppError::Internal(Error::MissingSessionLayer(msg)))?;

        let user_id: i64 = session
            .get(SESSION_USER_ID_KEY)
            .await
            .map_err(Error::Session)?
            .ok_or(AppError::Unauthorized)?;

        // ここで見つからない = セッションは生きているがユーザーが削除された、というケース。
        // クライアントからは「未ログイン」と区別する意味が無いので同じ401にする。
        let user = sqlx::query_as!(
            UserRow,
            r#"SELECT id as "id!", username, password_hash, role as "role: Role",
                      recovery_code_hash IS NOT NULL as "has_recovery_code!: bool"
               FROM users WHERE id = ?"#,
            user_id
        )
        .fetch_optional(&state.pool)
        .await?;

        let Some(user) = user else {
            // ユーザーが削除された後の残留セッション。ここで破棄しておく。
            session.flush().await.map_err(Error::Session)?;
            return Err(AppError::Unauthorized);
        };

        Ok(AuthUser {
            id: user.id,
            username: user.username,
            role: user.role,
            has_recovery_code: user.has_recovery_code,
        })
    }
}

/// 閲覧者を表すextractor。未ログインでもエラーにせず `Anonymous` を返す
/// (→ docs/access.md「匿名閲覧の受け口」)。閲覧系ハンドラはこれを受け取り、対象コンテンツの
/// 閲覧レベルと突き合わせて401を返すかどうかを自分で決める。
///
/// `AuthUser` の抽出失敗のうち「未ログイン」(`AppError::Unauthorized`)だけを
/// `Anonymous` に落とす。セッションレイヤーの設定ミス(`Internal`)やDB障害
/// (`Database`)はそのまま伝播させる: これらを匿名扱いにまとめると、障害時に
/// ログイン済みユーザーが黙って匿名の見え方に落ちてしまう。
pub enum Viewer {
    Anonymous,
    User(AuthUser),
}

impl Viewer {
    /// ログイン済みかどうか。閲覧レベルの判定に使う。
    pub fn is_authenticated(&self) -> bool {
        matches!(self, Viewer::User(_))
    }
}

impl FromRequestParts<AppState> for Viewer {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match AuthUser::from_request_parts(parts, state).await {
            Ok(user) => Ok(Viewer::User(user)),
            Err(AppError::Unauthorized) => Ok(Viewer::Anonymous),
            Err(err) => Err(err),
        }
    }
}

/// 管理者ロールを持つログイン済みユーザーを表すextractor。ハンドラの引数に
/// `AdminUser` を書くだけで、未ログインなら401、ログイン済みでもadminでなければ403になる。
/// `AuthUser` を内包し、`Deref` 経由でフィールドにそのままアクセスできる。
pub struct AdminUser(pub AuthUser);

impl std::ops::Deref for AdminUser {
    type Target = AuthUser;

    fn deref(&self) -> &AuthUser {
        &self.0
    }
}

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        if !user.is_admin() {
            return Err(AppError::Forbidden);
        }
        Ok(AdminUser(user))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_then_verify_roundtrip() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password("correct horse battery staple", &hash).unwrap());
        assert!(!verify_password("wrong password", &hash).unwrap());
    }

    #[test]
    fn generated_recovery_code_is_grouped_and_normalizes_to_itself() {
        let code = generate_recovery_code().expect("OS の乱数源から読めるはず");
        let groups: Vec<&str> = code.split('-').collect();
        assert_eq!(groups.len(), 4, "{code}");
        assert!(groups.iter().all(|g| g.len() == 5), "{code}");
        assert_eq!(
            normalize_recovery_code(&code).expect("作ったコードは正規化できる形のはず"),
            code.replace('-', "")
        );
    }

    #[test]
    fn recovery_code_normalization() {
        assert_eq!(
            normalize_recovery_code(" abcde-fghjk mnpqr-stvwo ").as_deref(),
            Some("ABCDEFGHJKMNPQRSTVW0")
        );
        assert_eq!(
            normalize_recovery_code("i1l00000000000000000").as_deref(),
            Some("11100000000000000000")
        );
        // `U` は Crockford の base32 に無い。長さが違うものも通さない。
        assert_eq!(normalize_recovery_code("U0000000000000000000"), None);
        assert_eq!(normalize_recovery_code("0000"), None);
        assert_eq!(normalize_recovery_code(""), None);
    }

    #[test]
    fn verify_against_garbage_hash_is_error() {
        let result = verify_password("anything", "not a valid phc string");
        assert!(result.is_err());
    }

    #[test]
    fn dummy_hash_never_verifies() {
        // DUMMY_HASH に対応する平文は存在しないはず、というテスト
        // (誤って本物のハッシュに書き換えてしまった場合の回帰防止)。
        assert!(!verify_password("password", &DUMMY_HASH).unwrap());
        assert!(!verify_password("", &DUMMY_HASH).unwrap());
    }

    #[sqlx::test]
    async fn authenticate_succeeds_with_correct_password(pool: SqlitePool) {
        insert_user(&pool, "alice", "correct-password").await;

        let user = authenticate(&pool, "alice", "correct-password")
            .await
            .expect("authenticate should not error");

        let user = user.expect("correct credentials should authenticate");
        assert_eq!(user.username, "alice");
    }

    #[sqlx::test]
    async fn authenticate_fails_with_wrong_password(pool: SqlitePool) {
        insert_user(&pool, "alice", "correct-password").await;

        let user = authenticate(&pool, "alice", "wrong-password")
            .await
            .expect("authenticate should not error");

        assert!(user.is_none());
    }

    #[sqlx::test]
    async fn authenticate_succeeds_with_different_case_username(pool: SqlitePool) {
        insert_user(&pool, "alice", "correct-password").await;

        let user = authenticate(&pool, "ALICE", "correct-password")
            .await
            .expect("authenticate should not error");

        let user = user.expect("case-insensitive match should authenticate");
        // DB上の表記(登録時のcase)がそのまま返る。
        assert_eq!(user.username, "alice");
    }

    #[sqlx::test]
    async fn authenticate_fails_for_unknown_user(pool: SqlitePool) {
        let user = authenticate(&pool, "no-such-user", "password")
            .await
            .expect("authenticate should not error");

        assert!(user.is_none());
    }

    /// ユーザー名は大文字小文字を区別せずに重なりを調べる (`users_username_nocase` の一意インデックス)。
    #[sqlx::test]
    async fn create_user_rejects_a_username_differing_only_in_case(pool: SqlitePool) {
        create_user(&pool, "alice", "password", Role::User, None)
            .await
            .expect("最初のユーザーは作れるはず");

        let result = create_user(&pool, "ALICE", "password", Role::User, None).await;

        assert!(
            matches!(result, Err(CreateUserError::UsernameTaken(_))),
            "{:?}",
            result.err()
        );
    }

    #[sqlx::test]
    async fn rename_user_changes_the_name_used_to_authenticate(pool: SqlitePool) {
        let alice = create_user(&pool, "alice", "password", Role::User, None)
            .await
            .expect("作れるはず");

        rename_user(&pool, alice.id, "alicia")
            .await
            .expect("変えられるはず");

        let user = authenticate(&pool, "alicia", "password")
            .await
            .expect("照合で失敗しないはず");
        assert_eq!(user.map(|u| u.username).as_deref(), Some("alicia"));
    }

    /// 大文字小文字だけの違いもほかの人の名前とは重ねられないが、自分の名前の大文字小文字は変えられる。
    #[sqlx::test]
    async fn rename_user_rejects_another_users_name_ignoring_case(pool: SqlitePool) {
        create_user(&pool, "alice", "password", Role::User, None)
            .await
            .expect("作れるはず");
        let bob = create_user(&pool, "bob", "password", Role::User, None)
            .await
            .expect("作れるはず");

        let result = rename_user(&pool, bob.id, "ALICE").await;
        assert!(
            matches!(&result, Err(RenameUserError::UsernameTaken(name)) if name == "ALICE"),
            "{:?}",
            result.err()
        );

        rename_user(&pool, bob.id, "Bob")
            .await
            .expect("自分の名前の大文字小文字は変えられるはず");
    }

    #[sqlx::test]
    async fn rename_user_of_missing_user_is_not_found(pool: SqlitePool) {
        let result = rename_user(&pool, 9999, "nobody").await;

        assert!(
            matches!(result, Err(RenameUserError::NotFound)),
            "{:?}",
            result.err()
        );
    }

    /// 入力の区切り・空白・大文字小文字・紛らわしい字の違いを許し、ユーザー名も大文字小文字を区別しない。
    /// 形の揃え方の場合分けは `recovery_code_normalization` で見る。
    #[sqlx::test]
    async fn verify_recovery_code_accepts_formatting_differences(pool: SqlitePool) {
        insert_user(&pool, "admin", "password").await;
        let (code, hash) = new_recovery_code().await.expect("コードを作れるはず");
        sqlx::query("UPDATE users SET recovery_code_hash = ? WHERE username = 'admin'")
            .bind(&hash)
            .execute(&pool)
            .await
            .expect("コードを保存できなかった");
        let typed: String = code
            .chars()
            .filter(|c| *c != '-')
            .map(|c| match c {
                '0' => 'o',
                '1' => 'l',
                c => c.to_ascii_lowercase(),
            })
            .collect();

        let verified = verify_recovery_code(&pool, "ADMIN", &format!(" {typed} "))
            .await
            .expect("照合で失敗しないはず");

        assert!(verified.is_some(), "{code} / {typed}");
    }

    async fn insert_user(pool: &SqlitePool, username: &str, password: &str) {
        let hash = hash_password(password).unwrap();
        sqlx::query!(
            "INSERT INTO users (username, password_hash) VALUES (?, ?)",
            username,
            hash
        )
        .execute(pool)
        .await
        .expect("failed to insert test user");
    }

    mod rate_limiter {
        use super::*;

        #[test]
        fn allows_up_to_max_attempts_then_blocks() {
            let limiter = LoginRateLimiter::new(3, 100, Duration::from_secs(60));

            assert!(limiter.try_acquire("admin"));
            assert!(limiter.try_acquire("admin"));
            assert!(limiter.try_acquire("admin"));
            assert!(!limiter.try_acquire("admin"));
        }

        #[test]
        fn tracks_each_username_independently() {
            let limiter = LoginRateLimiter::new(1, 100, Duration::from_secs(60));

            assert!(limiter.try_acquire("admin"));
            assert!(!limiter.try_acquire("admin"));
            assert!(limiter.try_acquire("other"));
        }

        #[test]
        fn treats_username_case_variants_as_the_same_key() {
            // ログイン照合(COLLATE NOCASE)と揃え、大文字小文字を変えただけの試行が
            // 別バケットとして扱われ制限を回避できてしまわないことを確認する。
            let limiter = LoginRateLimiter::new(1, 100, Duration::from_secs(60));

            assert!(limiter.try_acquire("Admin"));
            assert!(!limiter.try_acquire("admin"));
            assert!(!limiter.try_acquire("ADMIN"));
        }

        #[test]
        fn global_limit_blocks_even_with_distinct_usernames() {
            let limiter = LoginRateLimiter::new(100, 3, Duration::from_secs(60));

            assert!(limiter.try_acquire("user-1"));
            assert!(limiter.try_acquire("user-2"));
            assert!(limiter.try_acquire("user-3"));
            // 別のユーザー名でも、グローバル上限を超えていれば拒否される。
            assert!(!limiter.try_acquire("user-4"));
        }

        #[test]
        fn does_not_track_more_than_max_ids() {
            let limiter = LoginRateLimiter {
                max_tracked_ids: 2,
                ..LoginRateLimiter::new(100, 100, Duration::from_secs(60))
            };

            assert!(limiter.try_acquire("user-1"));
            assert!(limiter.try_acquire("user-2"));
            // 追跡上限に達しているため、3件目の新規ユーザー名は拒否される。
            assert!(!limiter.try_acquire("user-3"));
            // 既に追跡中のユーザー名は引き続き許可される。
            assert!(limiter.try_acquire("user-1"));
        }

        #[test]
        fn release_frees_up_a_slot_for_both_user_and_global() {
            let limiter = LoginRateLimiter::new(1, 1, Duration::from_secs(60));

            assert!(limiter.try_acquire("admin"));
            assert!(!limiter.try_acquire("admin")); // 上限到達(ユーザー単位・グローバル単位とも1)

            limiter.release("admin");

            // 解放されているので、成功ログインを繰り返しても再びブロックされない。
            assert!(limiter.try_acquire("admin"));
        }

        #[test]
        fn release_on_untracked_username_is_a_no_op() {
            let limiter = LoginRateLimiter::new(1, 100, Duration::from_secs(60));

            // 試行を記録していないユーザー名に対して呼んでもpanicしない。
            limiter.release("never-tried");

            assert!(limiter.try_acquire("never-tried"));
        }
    }
}
