//! AI の機能の土台 (→ docs/ai.md)。PC の登録の状態を持ち、窓口 (→ docs/account-server.md) を呼び、
//! 誰がどれだけ使ったかを記録する。画面から呼ぶ口は `api::ai`。

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use ureq::Agent;

use crate::auth::AuthUser;
use crate::config::{create_owner_only_dir, replace_owner_only_file};
use crate::error::{AppError, run_blocking};
use crate::state::AppState;

/// 配る版がつなぐ窓口。
#[cfg(not(debug_assertions))]
pub const ACCOUNT_SERVER_URL: &str = "https://weblav.amiiby.com";

/// 開発版がつなぐ、手元で動かす窓口 (→ `just dev-account-server`)。本番の残りを開発で減らさないため。
#[cfg(debug_assertions)]
pub const ACCOUNT_SERVER_URL: &str = "http://127.0.0.1:8797";

/// 登録と残りの問い合わせを待つ上限。窓口が応えないときに、設定の画面を長く待たせないように。
const ACCOUNT_TIMEOUT: Duration = Duration::from_secs(10);

/// AI の返事を待つ上限。窓口は 50 秒で諦めるので、それより長くして、窓口の返事 (使った量を含む) を受け取る。
const AI_TIMEOUT: Duration = Duration::from_secs(60);

/// 窓口が受ける、指示文と文の合計の上限 (UTF-16 の単位)。送る前に確かめて、送る時間を使わずに断る。
const MAX_INPUT_UNITS: usize = 200_000;

/// 窓口が使った量を返すヘッダー。1回の購入で付く量を 1,000,000 とした整数。
const USED_HEADER: &str = "x-weblav-used";

/// PC の登録の状態。ファイルが無ければ未登録。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Account {
    /// 登録の途中。窓口の画面で認めるのを待っている。
    Pending {
        /// 検証用の値 (32 バイトの乱数の16進)。窓口にはハッシュだけを見せ、トークンに替えるときに送る。
        code_verifier: String,
        /// 始めた時刻 (unix 秒)。
        started_at: i64,
    },
    /// 登録済み。
    Linked {
        /// アプリ用のトークン。
        token: String,
    },
}

/// 窓口で見た、アカウントの残り。
#[derive(Debug, Deserialize)]
pub struct Balance {
    pub email: Option<String>,
    pub remaining_percent: f64,
}

/// AI に送る中身。
pub struct Prompt {
    /// 指示文。
    pub system: Option<String>,
    /// 文。
    pub user: String,
    /// 添付。窓口が受けるのは PDF・PNG・JPEG・WebP を1つ。
    pub attachment: Option<Attachment>,
}

pub struct Attachment {
    pub mime_type: String,
    pub data: Vec<u8>,
}

/// 使った機能。`ai_usage.kind` に入る。
#[derive(Debug, Clone, Copy, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
pub enum UsageKind {
    /// アーカイブの軸の定義。
    Axes,
}

/// 窓口の AI の返事。失敗でも、AI が使った量を返していれば `used` がある。
struct Relayed {
    answer: Result<String, AppError>,
    used: Option<i64>,
}

/// 動いている WebLAV の、PC の登録の状態と窓口の場所。起動したときにファイルを読む。
///
/// ファイルの読み書きと窓口の呼び出しは同期なので、メソッドは `run_blocking` に載せて呼ぶ。
pub struct Ai {
    path: PathBuf,
    server: String,
    // ファイルの書き換えの間も持ち続ける。ファイルとこの値が食い違わないように。窓口を呼ぶ間は持たない。
    account: Mutex<Option<Account>>,
}

impl Ai {
    /// `path` の登録の状態を読む。読めない形のファイルは未登録として扱う (登録し直せば上書きされる)。
    pub fn load(path: PathBuf, server: String) -> Self {
        let account = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .inspect_err(|err| {
                    tracing::warn!(path = %path.display(), %err, "failed to parse the AI account file; treating as unlinked");
                })
                .ok(),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                tracing::warn!(path = %path.display(), %err, "failed to read the AI account file; treating as unlinked");
                None
            }
        };
        Self {
            path,
            server,
            account: Mutex::new(account),
        }
    }

    pub fn server(&self) -> &str {
        &self.server
    }

    pub fn account(&self) -> Option<Account> {
        self.lock().clone()
    }

    pub fn is_linked(&self) -> bool {
        matches!(*self.lock(), Some(Account::Linked { .. }))
    }

    fn lock(&self) -> MutexGuard<'_, Option<Account>> {
        self.account.lock().expect("ai account lock poisoned")
    }

    /// ファイルを書き換えてから、メモリの値を変える。書けなければどちらも変えない。
    fn store(&self, slot: &mut Option<Account>, next: Option<Account>) -> std::io::Result<()> {
        match &next {
            Some(account) => {
                if let Some(parent) = self.path.parent() {
                    create_owner_only_dir(parent)?;
                }
                let bytes = serde_json::to_vec(account).map_err(std::io::Error::other)?;
                replace_owner_only_file(
                    &self.path.with_extension("tmp"),
                    &self.path,
                    &bytes,
                    false,
                )?;
            }
            None => match std::fs::remove_file(&self.path) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err),
                _ => {}
            },
        }
        *slot = next;
        Ok(())
    }

    /// 登録を始め、検証用の値を返す。途中のものがあれば、その値を返す
    /// (窓口の画面を開いた後に押し直しても、開いた画面で認められるように)。登録済みなら `None`。
    pub fn begin_link(&self) -> std::io::Result<Option<String>> {
        let mut account = self.lock();
        match &*account {
            Some(Account::Linked { .. }) => Ok(None),
            Some(Account::Pending { code_verifier, .. }) => Ok(Some(code_verifier.clone())),
            None => {
                let code_verifier = crate::api::random_hex::<32>()?;
                let pending = Account::Pending {
                    code_verifier: code_verifier.clone(),
                    started_at: time::OffsetDateTime::now_utc().unix_timestamp(),
                };
                self.store(&mut account, Some(pending))?;
                Ok(Some(code_verifier))
            }
        }
    }

    /// 登録の途中なら、窓口に認めたかを問い合わせ、認めていればトークンを保存する。
    pub fn poll_link(&self) -> Result<(), AppError> {
        let Some(Account::Pending { code_verifier, .. }) = self.account() else {
            return Ok(());
        };
        let Some(token) = exchange(&self.server, &code_verifier)? else {
            return Ok(());
        };
        let mut account = self.lock();
        let still_pending = matches!(
            &*account,
            Some(Account::Pending { code_verifier: current, .. }) if *current == code_verifier
        );
        if still_pending {
            self.store(&mut account, Some(Account::Linked { token }))?;
        } else {
            // 問い合わせの間に取り消された。受け取ったトークンは使わないので、窓口に残さない。
            drop(account);
            self.revoke_quietly(&token);
        }
        Ok(())
    }

    /// 登録の途中を取り消す。
    pub fn cancel_link(&self) -> std::io::Result<()> {
        let mut account = self.lock();
        if matches!(*account, Some(Account::Pending { .. })) {
            self.store(&mut account, None)?;
        }
        Ok(())
    }

    /// 登録を解除する。窓口につながらなくても、手元のトークンは消す。
    pub fn unlink(&self) -> std::io::Result<()> {
        let token = {
            let mut account = self.lock();
            let Some(Account::Linked { token }) = account.clone() else {
                return Ok(());
            };
            self.store(&mut account, None)?;
            token
        };
        self.revoke_quietly(&token);
        Ok(())
    }

    fn revoke_quietly(&self, token: &str) {
        if let Err(err) = revoke(&self.server, token) {
            tracing::warn!(%err, "failed to revoke the app token on the account server");
        }
    }

    /// 窓口がトークンを断った (窓口の側で登録を外された) とき、手元のトークンを消して未登録に戻す。
    fn forget_if_rejected<T>(&self, token: &str, result: &Result<T, AppError>) {
        if !matches!(result, Err(AppError::AiNotLinked)) {
            return;
        }
        let mut account = self.lock();
        // 問い合わせの間に登録し直していたら、新しいトークンは消さない。
        if matches!(&*account, Some(Account::Linked { token: current }) if current == token)
            && let Err(err) = self.store(&mut account, None)
        {
            tracing::warn!(%err, "failed to remove the rejected app token");
        }
    }

    /// アカウントのメールアドレスと残り。
    pub fn balance(&self) -> Result<Balance, AppError> {
        let Some(Account::Linked { token }) = self.account() else {
            return Err(AppError::AiNotLinked);
        };
        let result = get_balance(&self.server, &token);
        self.forget_if_rejected(&token, &result);
        result
    }

    fn relay(&self, prompt: &Prompt) -> Relayed {
        let Some(Account::Linked { token }) = self.account() else {
            return Relayed {
                answer: Err(AppError::AiNotLinked),
                used: None,
            };
        };
        let relayed = post_ai(&self.server, &token, prompt);
        self.forget_if_rejected(&token, &relayed.answer);
        relayed
    }
}

/// `user` が AI を使えるか (→ docs/ai.md「誰が使えるか」)。管理者は常に使え、編集者は印が付いていれば使える。
pub async fn is_allowed(pool: &SqlitePool, user: &AuthUser) -> Result<bool, sqlx::Error> {
    if user.is_admin() {
        return Ok(true);
    }
    let allowed = sqlx::query_scalar!(
        r#"SELECT ai_allowed as "ai_allowed: bool" FROM users WHERE id = ?"#,
        user.id
    )
    .fetch_optional(pool)
    .await?;
    Ok(allowed.unwrap_or(false))
}

/// 窓口を通して AI に送り、答えの文字列を返す。使った量は `user` の分として記録する。
pub async fn generate(
    state: &AppState,
    user: &AuthUser,
    kind: UsageKind,
    prompt: Prompt,
) -> Result<String, AppError> {
    if !is_allowed(&state.pool, user).await? {
        return Err(AppError::AiNotAllowed);
    }
    if !state.ai.is_linked() {
        return Err(AppError::AiNotLinked);
    }
    if input_units(&prompt) > MAX_INPUT_UNITS {
        return Err(AppError::AiTooLarge);
    }
    let ai = Arc::clone(&state.ai);
    let relayed = run_blocking(move || ai.relay(&prompt)).await?;
    if let Some(used) = relayed.used {
        // 記録できなくても、残りを使って届いた答えは返す。
        if let Err(err) = sqlx::query!(
            "INSERT INTO ai_usage (user_id, username, kind, used) VALUES (?, ?, ?, ?)",
            user.id,
            user.username,
            kind,
            used
        )
        .execute(&state.pool)
        .await
        {
            tracing::error!(%err, used, "failed to record AI usage");
        }
    }
    relayed.answer
}

/// 窓口が数えるのと同じ、指示文と文の合計の長さ (UTF-16 の単位)。
fn input_units(prompt: &Prompt) -> usize {
    let units = |text: &str| text.encode_utf16().count();
    units(&prompt.user) + prompt.system.as_deref().map_or(0, units)
}

/// 管理者が開いて PC の登録を認める、窓口の画面の URL。
/// 窓口には検証用の値のハッシュだけを見せる。URL を見た人がトークンに替えられないように。
pub fn link_url(server: &str, code_verifier: &str, site_name: &str, pc_name: &str) -> String {
    let challenge = crate::api::hex_encode(&Sha256::digest(code_verifier.as_bytes()));
    let encode = |value: &str| utf8_percent_encode(value, NON_ALPHANUMERIC).to_string();
    format!(
        "{server}/account/link?challenge={challenge}&name={}&pc={}",
        encode(site_name),
        encode(pc_name)
    )
}

/// 管理者が残りを足す、窓口のアカウントのページ。
pub fn account_url(server: &str) -> String {
    format!("{server}/account/")
}

fn agent(timeout: Duration) -> Agent {
    Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .user_agent(concat!("WebLAV/", env!("CARGO_PKG_VERSION")))
        .tls_config(crate::api::native_tls_config())
        .build()
        .into()
}

/// 窓口の返事。
struct Reply {
    status: u16,
    used: Option<i64>,
    body: String,
}

fn read(
    url: &str,
    response: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<Reply, AppError> {
    let unreachable = |err: ureq::Error| AppError::AccountServer(format!("{url}: {err}"));
    let mut response = response.map_err(unreachable)?;
    let used = response
        .headers()
        .get(USED_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|used| *used >= 0);
    Ok(Reply {
        status: response.status().as_u16(),
        used,
        body: response.body_mut().read_to_string().map_err(unreachable)?,
    })
}

/// 窓口が断った理由を、画面に返すエラーに分ける (→ docs/ai.md「エラーの code」)。
fn failure(url: &str, reply: &Reply) -> AppError {
    #[derive(Deserialize)]
    struct Refusal {
        error: String,
    }
    match reply.status {
        401 => AppError::AiNotLinked,
        402 => AppError::AiNoCredit,
        413 => AppError::AiTooLarge,
        429 => AppError::AiRateLimited,
        // 窓口の手前 (CDN など) が返す 502 と分けるため、窓口が付ける印も見る。
        502 if serde_json::from_str::<Refusal>(&reply.body)
            .is_ok_and(|refusal| refusal.error == "upstream") =>
        {
            AppError::AiUpstream
        }
        status => AppError::AccountServer(format!("{url}: status {status}")),
    }
}

fn malformed(url: &str, err: impl std::fmt::Display) -> AppError {
    AppError::AccountServer(format!("{url}: {err}"))
}

/// 検証用の値をトークンに替える。まだ認められていなければ `None`。
fn exchange(server: &str, code_verifier: &str) -> Result<Option<String>, AppError> {
    #[derive(Deserialize)]
    struct Issued {
        token: String,
    }
    let url = format!("{server}/v1/links/token");
    let body = serde_json::json!({ "code_verifier": code_verifier }).to_string();
    let reply = read(
        &url,
        agent(ACCOUNT_TIMEOUT)
            .post(&url)
            .header("content-type", "application/json")
            .send(&body),
    )?;
    match reply.status {
        202 => Ok(None),
        200 => serde_json::from_str::<Issued>(&reply.body)
            .map(|issued| Some(issued.token))
            .map_err(|err| malformed(&url, err)),
        _ => Err(failure(&url, &reply)),
    }
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

fn get_balance(server: &str, token: &str) -> Result<Balance, AppError> {
    let url = format!("{server}/v1/balance");
    let reply = read(
        &url,
        agent(ACCOUNT_TIMEOUT)
            .get(&url)
            .header("authorization", bearer(token))
            .call(),
    )?;
    if reply.status != 200 {
        return Err(failure(&url, &reply));
    }
    serde_json::from_str(&reply.body).map_err(|err| malformed(&url, err))
}

fn revoke(server: &str, token: &str) -> Result<(), AppError> {
    let url = format!("{server}/v1/token");
    let reply = read(
        &url,
        agent(ACCOUNT_TIMEOUT)
            .delete(&url)
            .header("authorization", bearer(token))
            .call(),
    )?;
    if reply.status != 204 {
        return Err(failure(&url, &reply));
    }
    Ok(())
}

#[derive(Serialize)]
struct AiBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a str>,
    user: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    attachments: Option<[AttachmentBody<'a>; 1]>,
}

#[derive(Serialize)]
struct AttachmentBody<'a> {
    mime_type: &'a str,
    /// base64。
    data: String,
}

fn post_ai(server: &str, token: &str, prompt: &Prompt) -> Relayed {
    let url = format!("{server}/v1/ai");
    let body = AiBody {
        system: prompt.system.as_deref(),
        user: &prompt.user,
        attachments: prompt.attachment.as_ref().map(|attachment| {
            [AttachmentBody {
                mime_type: &attachment.mime_type,
                data: STANDARD.encode(&attachment.data),
            }]
        }),
    };
    let reply = serde_json::to_string(&body)
        .map_err(|err| malformed(&url, err))
        .and_then(|body| {
            read(
                &url,
                agent(AI_TIMEOUT)
                    .post(&url)
                    .header("content-type", "application/json")
                    .header("authorization", bearer(token))
                    .send(&body),
            )
        });
    let reply = match reply {
        Ok(reply) => reply,
        Err(err) => {
            return Relayed {
                answer: Err(err),
                used: None,
            };
        }
    };
    let answer = if reply.status == 200 {
        // 答えの文が無い返事 (安全のための打ち切りなど) は、AI の側の失敗として伝える。
        answer_text(&reply.body).ok_or(AppError::AiUpstream)
    } else {
        Err(failure(&url, &reply))
    };
    Relayed {
        answer,
        used: reply.used,
    }
}

/// Gemini の Interactions API の返事から、答えの文を取り出す。
/// `steps` には考えた過程なども並ぶので、`model_output` の文だけをつなげる。文が無ければ `None`。
fn answer_text(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let text: String = json["steps"]
        .as_array()?
        .iter()
        .filter(|step| step["type"] == "model_output")
        .filter_map(|step| step["content"].as_array())
        .flatten()
        .filter_map(|content| content["text"].as_str())
        .collect();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// どこにもつながない窓口。ファイルの読み書きだけを確かめるテスト用。
    const NO_SERVER: &str = "http://127.0.0.1:1";

    fn ai_in(dir: &std::path::Path) -> Ai {
        Ai::load(dir.join("ai-account.json"), NO_SERVER.to_string())
    }

    #[test]
    fn begin_link_keeps_the_pending_verifier_across_restarts() {
        let dir = crate::test_support::TempDir::new("ai-begin-link");
        let ai = ai_in(&dir);
        assert_eq!(ai.account(), None);

        let verifier = ai
            .begin_link()
            .expect("登録を始められなかった")
            .expect("未登録なのに検証用の値が返らなかった");
        assert_eq!(verifier.len(), 64);
        assert!(verifier.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(
            ai.begin_link().expect("登録を始め直せなかった"),
            Some(verifier.clone()),
            "途中のものがあれば同じ値を返す"
        );

        let reloaded = ai_in(&dir);
        assert!(matches!(
            reloaded.account(),
            Some(Account::Pending { code_verifier, .. }) if code_verifier == verifier
        ));
    }

    #[cfg(unix)]
    #[test]
    fn account_file_is_readable_only_by_the_owner() {
        use std::os::unix::fs::PermissionsExt;

        let dir = crate::test_support::TempDir::new("ai-file-mode");
        let ai = ai_in(&dir);
        ai.begin_link().expect("登録を始められなかった");
        let mode = std::fs::metadata(dir.join("ai-account.json"))
            .expect("登録の状態のファイルが無い")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn cancel_link_removes_the_file() {
        let dir = crate::test_support::TempDir::new("ai-cancel-link");
        let ai = ai_in(&dir);
        ai.begin_link().expect("登録を始められなかった");
        ai.cancel_link().expect("取り消せなかった");
        assert_eq!(ai.account(), None);
        assert!(!dir.join("ai-account.json").exists());
    }

    #[test]
    fn begin_link_returns_none_when_linked() {
        let dir = crate::test_support::TempDir::new("ai-begin-linked");
        std::fs::write(
            dir.join("ai-account.json"),
            r#"{"state":"linked","token":"abc"}"#,
        )
        .expect("登録の状態を書けなかった");
        let ai = ai_in(&dir);
        assert!(ai.is_linked());
        assert_eq!(ai.begin_link().expect("登録を始められなかった"), None);
        ai.cancel_link().expect("取り消せなかった");
        assert!(ai.is_linked(), "登録の途中の取り消しは、登録済みを消さない");
    }

    #[test]
    fn unreadable_account_file_is_treated_as_unlinked() {
        let dir = crate::test_support::TempDir::new("ai-broken-file");
        std::fs::write(dir.join("ai-account.json"), "{").expect("登録の状態を書けなかった");
        assert_eq!(ai_in(&dir).account(), None);
    }

    #[test]
    fn link_url_carries_the_hash_of_the_verifier_and_escaped_names() {
        // SHA-256("abc") (FIPS 180-2 の例)。
        let url = link_url("https://example.com", "abc", "教室 A&B", "my-pc");
        assert_eq!(
            url,
            "https://example.com/account/link\
             ?challenge=ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\
             &name=%E6%95%99%E5%AE%A4%20A%26B&pc=my%2Dpc"
        );
    }

    #[test]
    fn answer_text_joins_only_the_model_output() {
        let body = r#"{"steps":[
            {"type":"thought","content":[{"type":"text","text":"考え中"}]},
            {"type":"model_output","content":[{"type":"text","text":"{\"axes\":"},{"type":"text","text":"[]}"}]},
            {"type":"model_output","content":[{"type":"image"}]}
        ],"usage":{}}"#;
        assert_eq!(answer_text(body).as_deref(), Some(r#"{"axes":[]}"#));
    }

    #[test]
    fn answer_text_is_none_without_text() {
        assert_eq!(answer_text(r#"{"steps":[]}"#), None);
        assert_eq!(answer_text(r#"{"status":"failed"}"#), None);
        assert_eq!(answer_text("not json"), None);
    }

    #[test]
    fn input_units_counts_utf16_units_of_system_and_user() {
        let prompt = Prompt {
            system: Some("ab".to_string()),
            // 絵文字は UTF-16 で2単位。窓口 (JavaScript の `length`) と同じ数え方にする。
            user: "あ😀".to_string(),
            attachment: None,
        };
        assert_eq!(input_units(&prompt), 5);
    }
}
