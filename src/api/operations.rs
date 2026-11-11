//! 操作の記録 (`operations`)。→ docs/operations.md
//!
//! 管理者が、誰がいつ何をしたかを後から確かめるためのもの。利用者の操作のうち、データベースか
//! 共有の中を書き換えるものと、ログインを残す。見るだけの操作と、自動で動く処理は残さない。
//! 記録に失敗しても操作そのものは止めない (記録のために、済んだ操作を失敗に見せないため)。

use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::{AdminUser, AuthUser};
use crate::error::AppError;
use crate::state::AppState;

/// 記録を残す日数。1年前の同じ時期の出来事 (年度替わりの整理など) まで遡れるようにする。
const RETENTION_DAYS: i64 = 365;

/// 古い記録を消しに行く間隔。日単位で残す長さを決めているので、1日ごとで足りる。
const PURGE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// 起動し直して途中で止まった操作の `error`。API のエラーではないので `ErrorCode` には無い。
const INTERRUPTED: &str = "interrupted";

/// 一覧の1回で返す既定の件数。管理画面の1画面で見る量。
const DEFAULT_PAGE_LIMIT: i64 = 100;
/// 一覧の1回で返す件数の上限。1回の応答を、画面が固まらない大きさに抑える。
const MAX_PAGE_LIMIT: i64 = 1000;

/// 操作の分類。画面の絞り込みに使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum OpCategory {
    /// ログイン・ログアウトと、自分のアカウントの変更。
    Auth,
    Contents,
    Users,
    /// 共有の中の操作と、共有の管理。
    Shares,
    /// サイト設定・サーバー設定・ログの設定・バックアップ。
    Settings,
}

/// 操作の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum Op {
    Login,
    Logout,
    ChangePassword,
    ChangeUsername,
    CreateRecoveryCode,
    /// リカバリコードでパスワードを再設定した。
    Recover,
    CreateContent,
    UpdateContent,
    ChangeCreator,
    DeleteContent,
    CreateAxis,
    UpdateAxis,
    DeleteAxis,
    /// 軸の値を書き換えた。
    ReplaceAxisValues,
    /// 軸の構成を読み込んだ (まとめて置き換えた)。
    ReplaceAxes,
    /// 一覧の表示の索引を読み直した。
    Rescan,
    CreateUser,
    ChangeRole,
    ResetPassword,
    RenameUser,
    DeleteUser,
    Upload,
    Mkdir,
    Rename,
    Move,
    Copy,
    Delete,
    /// フォルダーをホームに載せた (コンテンツを作った)。
    ListOnHome,
    CreateShare,
    RenameShare,
    ChangeShareAccess,
    /// 共有の場所を選び直した。
    MoveShare,
    DeleteShare,
    UpdateSiteSettings,
    UpdateServerSettings,
    UpdateLogSettings,
    DownloadBackup,
    RestoreBackup,
}

impl Op {
    pub fn category(self) -> OpCategory {
        use Op::*;
        match self {
            Login | Logout | ChangePassword | ChangeUsername | CreateRecoveryCode | Recover => {
                OpCategory::Auth
            }
            CreateContent | UpdateContent | ChangeCreator | DeleteContent | CreateAxis
            | UpdateAxis | DeleteAxis | ReplaceAxisValues | ReplaceAxes | Rescan => {
                OpCategory::Contents
            }
            CreateUser | ChangeRole | ResetPassword | RenameUser | DeleteUser => OpCategory::Users,
            Upload | Mkdir | Rename | Move | Copy | Delete | ListOnHome | CreateShare
            | RenameShare | ChangeShareAccess | MoveShare | DeleteShare => OpCategory::Shares,
            UpdateSiteSettings | UpdateServerSettings | UpdateLogSettings | DownloadBackup
            | RestoreBackup => OpCategory::Settings,
        }
    }
}

/// 操作の結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum OpResult {
    /// 裏で続いている (コピー・移動・削除)。
    Running,
    Succeeded,
    Failed,
    /// 途中で中止した。それまでに済んだ分はそのまま。
    Cancelled,
}

/// 共有の中の場所。記録には、そのときの共有の名前を写して残す。
#[derive(Debug, Clone)]
pub(crate) struct LoggedPlace {
    pub(crate) root_id: i64,
    pub(crate) root_name: String,
    pub(crate) path: String,
}

/// 書き込む1行。
#[derive(Debug, Clone)]
pub(crate) struct OpRecord {
    op: Op,
    user_id: Option<i64>,
    username: String,
    target: Option<(i64, String)>,
    renamed_to: Option<String>,
    part: Option<String>,
    fields: Option<Vec<String>>,
    root: Option<(i64, String)>,
    paths: Vec<String>,
    dest: Option<LoggedPlace>,
    item_count: Option<u64>,
    byte_count: Option<u64>,
    result: OpResult,
    error: Option<String>,
}

impl OpRecord {
    /// `user` がした操作の記録。対象・結果は後から足す。
    pub(crate) fn new(op: Op, user: &AuthUser) -> Self {
        Self::by(op, Some(user.id), &user.username)
    }

    /// ログインの前の操作のように、`AuthUser` の無い人の記録。
    pub(crate) fn by(op: Op, user_id: Option<i64>, username: &str) -> Self {
        Self {
            op,
            user_id,
            username: username.to_string(),
            target: None,
            renamed_to: None,
            part: None,
            fields: None,
            root: None,
            paths: Vec::new(),
            dest: None,
            item_count: None,
            byte_count: None,
            result: OpResult::Succeeded,
            error: None,
        }
    }

    /// `user` が共有 `root_id` の中の `paths` を扱った記録。置き先・件数・結果は後から足す。
    pub(crate) fn in_share(
        op: Op,
        user: &AuthUser,
        root_id: i64,
        root_name: &str,
        paths: Vec<String>,
    ) -> Self {
        let mut record = Self::new(op, user);
        record.root = Some((root_id, root_name.to_string()));
        record.paths = paths;
        record
    }

    /// 対象 (コンテンツ・ユーザー・共有など) の id と、そのときの名前。
    pub(crate) fn target(mut self, id: i64, name: &str) -> Self {
        self.target = Some((id, name.to_string()));
        self
    }

    /// 名前を変えた操作の、変えた後の名前。`target` には変える前の名前を渡す。
    pub(crate) fn renamed_to(mut self, name: &str) -> Self {
        self.renamed_to = Some(name.to_string());
        self
    }

    /// 対象の中の部分の名前 (アーカイブの軸の名前)。
    pub(crate) fn part(mut self, name: &str) -> Self {
        self.part = Some(name.to_string());
        self
    }

    /// 編集で変えた項目の名前 (API の項目名)。
    pub(crate) fn fields<S: Into<String>>(mut self, fields: impl IntoIterator<Item = S>) -> Self {
        self.fields = Some(fields.into_iter().map(Into::into).collect());
        self
    }

    pub(crate) fn dest(mut self, dest: LoggedPlace) -> Self {
        self.dest = Some(dest);
        self
    }

    pub(crate) fn counts(mut self, items: u64, bytes: u64) -> Self {
        self.item_count = Some(items);
        self.byte_count = Some(bytes);
        self
    }

    pub(crate) fn result(mut self, result: OpResult) -> Self {
        self.result = result;
        self
    }

    pub(crate) fn failed(mut self, error: &AppError) -> Self {
        self.result = OpResult::Failed;
        self.error = Some(error.code().to_string());
        self
    }
}

/// 1行書く。書いた行の id を返す (裏で続く操作は、終わったときに `finish` で結果を書く)。
pub(crate) async fn record(pool: &sqlx::SqlitePool, record: &OpRecord) -> Result<i64, AppError> {
    let paths = record
        .root
        .is_some()
        .then(|| to_json(&record.paths))
        .transpose()?;
    let fields = record.fields.as_ref().map(to_json).transpose()?;
    let category = record.op.category();
    let (target_id, target_name) = match &record.target {
        Some((id, name)) => (Some(*id), Some(name.as_str())),
        None => (None, None),
    };
    let (root_id, root_name) = match &record.root {
        Some((id, name)) => (Some(*id), Some(name.as_str())),
        None => (None, None),
    };
    let (dest_root_id, dest_root_name, dest_path) = match &record.dest {
        Some(dest) => (
            Some(dest.root_id),
            Some(dest.root_name.as_str()),
            Some(dest.path.as_str()),
        ),
        None => (None, None, None),
    };
    let item_count = record.item_count.map(saturating_i64);
    let byte_count = record.byte_count.map(saturating_i64);
    let id = sqlx::query_scalar!(
        r#"INSERT INTO operations
             (category, user_id, username, op, target_id, target_name, target_renamed_to, target_part,
              fields, root_id, root_name, paths, dest_root_id, dest_root_name, dest_path, item_count,
              byte_count, result, error)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
           RETURNING id as "id!""#,
        category,
        record.user_id,
        record.username,
        record.op,
        target_id,
        target_name,
        record.renamed_to,
        record.part,
        fields,
        root_id,
        root_name,
        paths,
        dest_root_id,
        dest_root_name,
        dest_path,
        item_count,
        byte_count,
        record.result,
        record.error,
    )
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// 前と後で値が変わった項目の名前 (JSON にしたときの項目名)。設定のように、項目をまとめて保存するものに使う。
pub(crate) fn changed_fields<T: Serialize>(before: &T, after: &T) -> Vec<String> {
    let (Ok(serde_json::Value::Object(before)), Ok(serde_json::Value::Object(after))) =
        (serde_json::to_value(before), serde_json::to_value(after))
    else {
        return Vec::new();
    };
    after
        .into_iter()
        .filter(|(key, value)| before.get(key.as_str()) != Some(value))
        .map(|(key, _)| key)
        .collect()
}

fn to_json<T: Serialize>(value: &T) -> Result<String, AppError> {
    serde_json::to_string(value).map_err(|err| AppError::Io(std::io::Error::other(err)))
}

/// `record` の失敗をログに出すだけにする版。記録できなくても操作は済んでいるため。
pub(crate) async fn record_or_warn(pool: &sqlx::SqlitePool, record: &OpRecord) -> Option<i64> {
    match self::record(pool, record).await {
        Ok(id) => Some(id),
        Err(err) => {
            tracing::warn!(error = %crate::error::error_chain(&err), op = ?record.op, "recording an operation failed");
            None
        }
    }
}

/// 裏で続いた操作 (`OpResult::Running` で書いた行) の結果を書く。失敗はログに出すだけ。
pub(crate) async fn finish(
    pool: &sqlx::SqlitePool,
    id: i64,
    result: OpResult,
    items: u64,
    bytes: u64,
    error: Option<&str>,
) {
    let items = saturating_i64(items);
    let bytes = saturating_i64(bytes);
    let updated = sqlx::query!(
        "UPDATE operations SET result = ?, item_count = ?, byte_count = ?, error = ? WHERE id = ?",
        result,
        items,
        bytes,
        error,
        id
    )
    .execute(pool)
    .await;
    if let Err(err) = updated {
        tracing::warn!(error = %crate::error::error_chain(&err), id, "recording the end of an operation failed");
    }
}

fn saturating_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// 起動したときの片付け。前に動いていたときに途中で止まった操作を失敗にし、古い記録を消す。
/// 裏の処理はメモリの中にしか無く、起動し直すと続きをしないため。
pub(crate) async fn on_startup(pool: &sqlx::SqlitePool) -> Result<(), AppError> {
    let interrupted = OpResult::Failed;
    let running = OpResult::Running;
    let code = INTERRUPTED;
    sqlx::query!(
        "UPDATE operations SET result = ?, error = ? WHERE result = ?",
        interrupted,
        code,
        running
    )
    .execute(pool)
    .await?;
    purge(pool).await
}

/// `RETENTION_DAYS` より古い記録を消す。
async fn purge(pool: &sqlx::SqlitePool) -> Result<(), AppError> {
    let modifier = format!("-{RETENTION_DAYS} days");
    sqlx::query!(
        "DELETE FROM operations WHERE at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?)",
        modifier
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// 1日ごとに古い記録を消す。起動したときの分は `on_startup` が済ませている。
pub(crate) fn spawn_purge(pool: sqlx::SqlitePool) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(PURGE_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // 1回目はすぐに来るので飛ばす (起動したときに消してある)。
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if let Err(err) = purge(&pool).await {
                tracing::warn!(error = %crate::error::error_chain(&err), "purging old operations failed");
            }
        }
    });
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(rename_all = "camelCase")]
struct OperationsQuery {
    /// これより前 (id が小さい) の記録を返す。前の応答の `nextBefore` を渡す。省略すると新しいものから。
    before: Option<i64>,
    /// 返す件数。既定 100、上限 1000。
    limit: Option<i64>,
    /// この分類だけを返す。
    category: Option<OpCategory>,
    /// このユーザーがした操作だけを返す。
    user_id: Option<i64>,
}

/// 記録の1行。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationResponse {
    id: i64,
    /// 操作した時刻 (UTC、ISO 8601)。
    at: String,
    category: OpCategory,
    /// 操作したユーザー。消したユーザーでも残る (`username` はそのときの名前)。
    /// ログインに失敗したときは打ち込まれた名前で、その名前のユーザーがいなければ `userId` は無い。
    user_id: Option<i64>,
    username: String,
    op: Op,
    /// 対象 (コンテンツ・ユーザー・共有など) の id と、そのときの名前。共有の中の操作には無い。
    target_id: Option<i64>,
    target_name: Option<String>,
    /// 名前を変えた操作の、変えた後の名前 (`targetName` は変える前の名前)。
    target_renamed_to: Option<String>,
    /// 対象の中の部分の名前 (アーカイブの軸の名前)。
    target_part: Option<String>,
    /// 編集で変えた項目の名前 (API の項目名)。編集でない操作には無い。
    fields: Option<Vec<String>>,
    /// 共有の中の操作の、扱った共有と、そのときの名前。
    share_id: Option<i64>,
    share_name: Option<String>,
    /// 扱った場所の、共有からの相対パス。共有の中の操作でなければ空。
    paths: Vec<String>,
    /// 移動・コピーの置き先のフォルダー、名前の変更の後の場所。
    dest_share_id: Option<i64>,
    dest_share_name: Option<String>,
    dest_path: Option<String>,
    /// 実際に置いた・消したファイル・フォルダーの数。飛ばしたもの、中止で置かなかったもの、失敗した1つ
    /// (別のディスクへの移動で、置けたが元を消せなかったものを含む) は数えない。
    /// 終わるまで分からないものと、共有の中の操作でないものは `None`。
    item_count: Option<i64>,
    /// 実際に置いたファイルの大きさの合計 (バイト)。
    byte_count: Option<i64>,
    result: OpResult,
    /// 失敗したときの API のエラーの code。
    error: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationsResponse {
    /// 新しい順。
    items: Vec<OperationResponse>,
    /// 続きがあれば、次に `before` へ渡す値。
    next_before: Option<i64>,
}

/// 操作の記録。新しい順。
#[utoipa::path(
    get,
    path = "/admin/operations",
    params(OperationsQuery),
    responses(
        (status = OK, body = OperationsResponse, description = "操作の記録"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
    )
)]
async fn list_operations(
    _admin: AdminUser,
    State(state): State<AppState>,
    Query(query): Query<OperationsQuery>,
) -> Result<Json<OperationsResponse>, AppError> {
    let limit = query
        .limit
        .unwrap_or(DEFAULT_PAGE_LIMIT)
        .clamp(1, MAX_PAGE_LIMIT);
    let before = query.before.unwrap_or(i64::MAX);
    // 続きがあるかを知るため、1件多く読む。
    let fetch = limit + 1;
    let rows = sqlx::query!(
        r#"SELECT id as "id!", at, category as "category: OpCategory", user_id, username,
                  op as "op: Op", target_id, target_name, target_renamed_to, target_part, fields, root_id, root_name, paths,
                  dest_root_id, dest_root_name, dest_path, item_count, byte_count,
                  result as "result: OpResult", error
           FROM operations
           WHERE id < ?1 AND (?2 IS NULL OR category = ?2) AND (?3 IS NULL OR user_id = ?3)
           ORDER BY id DESC LIMIT ?4"#,
        before,
        query.category,
        query.user_id,
        fetch
    )
    .fetch_all(&state.pool)
    .await?;
    let has_more = rows.len() as i64 > limit;
    // 書くのはこのモジュールだけなので、読めない JSON は壊れた行として空にする。
    let items: Vec<OperationResponse> = rows
        .into_iter()
        .take(usize::try_from(limit).unwrap_or(usize::MAX))
        .map(|row| OperationResponse {
            id: row.id,
            at: row.at,
            category: row.category,
            user_id: row.user_id,
            username: row.username,
            op: row.op,
            target_id: row.target_id,
            target_name: row.target_name,
            target_renamed_to: row.target_renamed_to,
            target_part: row.target_part,
            fields: row
                .fields
                .map(|fields| serde_json::from_str(&fields).unwrap_or_default()),
            share_id: row.root_id,
            share_name: row.root_name,
            paths: row
                .paths
                .map(|paths| serde_json::from_str(&paths).unwrap_or_default())
                .unwrap_or_default(),
            dest_share_id: row.dest_root_id,
            dest_share_name: row.dest_root_name,
            dest_path: row.dest_path,
            item_count: row.item_count,
            byte_count: row.byte_count,
            result: row.result,
            error: row.error,
        })
        .collect();
    let next_before = has_more.then(|| items.last().map(|item| item.id)).flatten();
    Ok(Json(OperationsResponse { items, next_before }))
}

pub(super) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(list_operations))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> AuthUser {
        AuthUser::for_test(1, crate::auth::Role::User)
    }

    fn in_share(op: Op, path: &str) -> OpRecord {
        OpRecord::in_share(op, &user(), 1, "教材", vec![path.to_string()])
    }

    async fn results(pool: &sqlx::SqlitePool) -> Vec<(i64, String, Option<String>)> {
        sqlx::query_as("SELECT id, result, error FROM operations ORDER BY id")
            .fetch_all(pool)
            .await
            .expect("読めなかった")
    }

    /// 途中で止まった操作は失敗にし、1年より古い記録は消す。
    #[sqlx::test]
    async fn startup_fails_interrupted_operations_and_purges_old_ones(pool: sqlx::SqlitePool) {
        let running = record(&pool, &in_share(Op::Copy, "a").result(OpResult::Running))
            .await
            .expect("書けなかった");
        let done = record(&pool, &in_share(Op::Mkdir, "b"))
            .await
            .expect("書けなかった");
        let old = record(&pool, &in_share(Op::Delete, "c"))
            .await
            .expect("書けなかった");
        sqlx::query("UPDATE operations SET at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-366 days') WHERE id = ?")
            .bind(old)
            .execute(&pool)
            .await
            .expect("書き換えられなかった");

        on_startup(&pool).await.expect("片付けられなかった");

        assert_eq!(
            results(&pool).await,
            vec![
                (
                    running,
                    "failed".to_string(),
                    Some("interrupted".to_string())
                ),
                (done, "succeeded".to_string(), None),
            ]
        );
    }

    #[sqlx::test]
    async fn finishing_writes_the_result_and_counts(pool: sqlx::SqlitePool) {
        let id = record(&pool, &in_share(Op::Move, "a").result(OpResult::Running))
            .await
            .expect("書けなかった");
        finish(&pool, id, OpResult::Cancelled, 3, 1024, None).await;
        let row: (String, i64, i64) =
            sqlx::query_as("SELECT result, item_count, byte_count FROM operations WHERE id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("読めなかった");
        assert_eq!(row, ("cancelled".to_string(), 3, 1024));
    }

    #[test]
    fn changed_fields_lists_only_the_fields_whose_values_differ() {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Settings {
            site_name: &'static str,
            home_heading: Option<&'static str>,
        }
        let before = Settings {
            site_name: "学校",
            home_heading: None,
        };
        let after = Settings {
            site_name: "学校",
            home_heading: Some("お知らせ"),
        };
        assert_eq!(changed_fields(&before, &after), vec!["homeHeading"]);
        assert!(changed_fields(&before, &before).is_empty());
    }

    /// 共有の中の操作でない行は、場所を持たず、分類を `op` から書く。
    #[sqlx::test]
    async fn an_operation_outside_shares_records_its_category_and_target(pool: sqlx::SqlitePool) {
        let id = record(
            &pool,
            &OpRecord::new(Op::UpdateContent, &user())
                .target(7, "時間割")
                .fields(vec!["title", "visibility"]),
        )
        .await
        .expect("書けなかった");
        let row: (String, i64, String, String, Option<String>) = sqlx::query_as(
            "SELECT category, target_id, target_name, fields, paths FROM operations WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("読めなかった");
        assert_eq!(
            row,
            (
                "contents".to_string(),
                7,
                "時間割".to_string(),
                r#"["title","visibility"]"#.to_string(),
                None
            )
        );
    }
}
