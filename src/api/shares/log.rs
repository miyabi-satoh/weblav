//! 共有の中のファイルを書き換えた操作の記録 (`share_operations`)。
//!
//! 管理者が、誰がいつ何を動かし・消したかを後から確かめるためのもの。見るだけの操作は残さない。
//! アップロードはファイルごとに1行、コピー・移動・削除は選んだものをまとめて1行にする。
//! コピー・移動の上書きで消したものは、それぞれ削除の1行として別に残す。
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

/// 操作の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum ShareOp {
    Upload,
    Mkdir,
    Rename,
    Move,
    Copy,
    Delete,
    /// フォルダーをホームに載せた (コンテンツを作った)。
    ListOnHome,
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
pub(super) struct LoggedPlace {
    pub(super) root_id: i64,
    pub(super) root_name: String,
    pub(super) path: String,
}

/// 書き込む1行。
#[derive(Debug, Clone)]
pub(super) struct OpRecord {
    pub(super) op: ShareOp,
    pub(super) user_id: i64,
    pub(super) username: String,
    pub(super) root_id: i64,
    pub(super) root_name: String,
    pub(super) paths: Vec<String>,
    pub(super) dest: Option<LoggedPlace>,
    pub(super) item_count: Option<u64>,
    pub(super) byte_count: Option<u64>,
    pub(super) result: OpResult,
    pub(super) error: Option<String>,
}

impl OpRecord {
    /// `user` が共有 `root_id` の中の `paths` を扱った記録。置き先・件数・結果は後から足す。
    pub(super) fn new(
        op: ShareOp,
        user: &AuthUser,
        root_id: i64,
        root_name: &str,
        paths: Vec<String>,
    ) -> Self {
        Self {
            op,
            user_id: user.id,
            username: user.username.clone(),
            root_id,
            root_name: root_name.to_string(),
            paths,
            dest: None,
            item_count: None,
            byte_count: None,
            result: OpResult::Succeeded,
            error: None,
        }
    }

    pub(super) fn dest(mut self, dest: LoggedPlace) -> Self {
        self.dest = Some(dest);
        self
    }

    pub(super) fn counts(mut self, items: u64, bytes: u64) -> Self {
        self.item_count = Some(items);
        self.byte_count = Some(bytes);
        self
    }

    pub(super) fn result(mut self, result: OpResult) -> Self {
        self.result = result;
        self
    }
}

/// 1行書く。書いた行の id を返す (裏で続く操作は、終わったときに `finish` で結果を書く)。
pub(super) async fn record(pool: &sqlx::SqlitePool, record: &OpRecord) -> Result<i64, AppError> {
    let paths = serde_json::to_string(&record.paths)
        .map_err(|err| AppError::Io(std::io::Error::other(err)))?;
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
        r#"INSERT INTO share_operations
             (user_id, username, op, root_id, root_name, paths, dest_root_id, dest_root_name,
              dest_path, item_count, byte_count, result, error)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
           RETURNING id as "id!""#,
        record.user_id,
        record.username,
        record.op,
        record.root_id,
        record.root_name,
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

/// `record` の失敗をログに出すだけにする版。記録できなくても操作は済んでいるため。
pub(super) async fn record_or_warn(pool: &sqlx::SqlitePool, record: &OpRecord) -> Option<i64> {
    match self::record(pool, record).await {
        Ok(id) => Some(id),
        Err(err) => {
            tracing::warn!(error = %crate::error::error_chain(&err), op = ?record.op, "recording a share operation failed");
            None
        }
    }
}

/// 裏で続いた操作 (`OpResult::Running` で書いた行) の結果を書く。失敗はログに出すだけ。
pub(super) async fn finish(
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
        "UPDATE share_operations SET result = ?, item_count = ?, byte_count = ?, error = ? WHERE id = ?",
        result,
        items,
        bytes,
        error,
        id
    )
    .execute(pool)
    .await;
    if let Err(err) = updated {
        tracing::warn!(error = %crate::error::error_chain(&err), id, "recording the end of a share operation failed");
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
        "UPDATE share_operations SET result = ?, error = ? WHERE result = ?",
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
        "DELETE FROM share_operations WHERE at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?)",
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
                tracing::warn!(error = %crate::error::error_chain(&err), "purging old share operations failed");
            }
        }
    });
}

#[derive(Debug, Deserialize, IntoParams)]
struct OperationsQuery {
    /// これより前 (id が小さい) の記録を返す。前の応答の `nextBefore` を渡す。省略すると新しいものから。
    before: Option<i64>,
    /// 返す件数。既定 100、上限 1000。
    limit: Option<i64>,
}

/// 記録の1行。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShareOperationResponse {
    id: i64,
    /// 操作した時刻 (UTC、ISO 8601)。
    at: String,
    /// 操作したユーザー。消したユーザーでも残る (`username` はそのときの名前)。
    user_id: Option<i64>,
    username: String,
    op: ShareOp,
    share_id: i64,
    /// そのときの共有の名前。
    share_name: String,
    /// 扱った場所の、共有からの相対パス。
    paths: Vec<String>,
    /// 移動・コピーの置き先のフォルダー、名前の変更の後の場所。
    dest_share_id: Option<i64>,
    dest_share_name: Option<String>,
    dest_path: Option<String>,
    /// 実際に置いた・消したファイル・フォルダーの数。飛ばしたもの、中止や失敗で置かなかったものは数えない。
    /// 終わるまで分からないものは `None`。
    item_count: Option<i64>,
    /// 実際に置いたファイルの大きさの合計 (バイト)。
    byte_count: Option<i64>,
    result: OpResult,
    /// 失敗したときの API のエラーの code。
    error: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShareOperationsResponse {
    /// 新しい順。
    items: Vec<ShareOperationResponse>,
    /// 続きがあれば、次に `before` へ渡す値。
    next_before: Option<i64>,
}

/// 共有の中のファイルを書き換えた操作の記録。新しい順。
#[utoipa::path(
    get,
    path = "/admin/share-operations",
    params(OperationsQuery),
    responses(
        (status = OK, body = ShareOperationsResponse, description = "操作の記録"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
    )
)]
async fn list_share_operations(
    _admin: AdminUser,
    State(state): State<AppState>,
    Query(query): Query<OperationsQuery>,
) -> Result<Json<ShareOperationsResponse>, AppError> {
    let limit = query
        .limit
        .unwrap_or(DEFAULT_PAGE_LIMIT)
        .clamp(1, MAX_PAGE_LIMIT);
    let before = query.before.unwrap_or(i64::MAX);
    // 続きがあるかを知るため、1件多く読む。
    let fetch = limit + 1;
    let rows = sqlx::query!(
        r#"SELECT id as "id!", at, user_id, username, op as "op: ShareOp", root_id, root_name, paths,
                  dest_root_id, dest_root_name, dest_path, item_count, byte_count,
                  result as "result: OpResult", error
           FROM share_operations WHERE id < ? ORDER BY id DESC LIMIT ?"#,
        before,
        fetch
    )
    .fetch_all(&state.pool)
    .await?;
    let has_more = rows.len() as i64 > limit;
    let items: Vec<ShareOperationResponse> = rows
        .into_iter()
        .take(usize::try_from(limit).unwrap_or(usize::MAX))
        .map(|row| ShareOperationResponse {
            id: row.id,
            at: row.at,
            user_id: row.user_id,
            username: row.username,
            op: row.op,
            share_id: row.root_id,
            share_name: row.root_name,
            // 書くのはこのモジュールだけなので、読めない値は壊れた行として空にする。
            paths: serde_json::from_str(&row.paths).unwrap_or_default(),
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
    Ok(Json(ShareOperationsResponse { items, next_before }))
}

pub(super) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(list_share_operations))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> AuthUser {
        AuthUser {
            id: 1,
            username: "taro".to_string(),
            role: crate::auth::Role::User,
            has_recovery_code: false,
        }
    }

    async fn results(pool: &sqlx::SqlitePool) -> Vec<(i64, String, Option<String>)> {
        sqlx::query_as("SELECT id, result, error FROM share_operations ORDER BY id")
            .fetch_all(pool)
            .await
            .expect("読めなかった")
    }

    /// 途中で止まった操作は失敗にし、1年より古い記録は消す。
    #[sqlx::test]
    async fn startup_fails_interrupted_operations_and_purges_old_ones(pool: sqlx::SqlitePool) {
        let running = record(
            &pool,
            &OpRecord::new(ShareOp::Copy, &user(), 1, "教材", vec!["a".to_string()])
                .result(OpResult::Running),
        )
        .await
        .expect("書けなかった");
        let done = record(
            &pool,
            &OpRecord::new(ShareOp::Mkdir, &user(), 1, "教材", vec!["b".to_string()]),
        )
        .await
        .expect("書けなかった");
        let old = record(
            &pool,
            &OpRecord::new(ShareOp::Delete, &user(), 1, "教材", vec!["c".to_string()]),
        )
        .await
        .expect("書けなかった");
        sqlx::query("UPDATE share_operations SET at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-366 days') WHERE id = ?")
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
        let id = record(
            &pool,
            &OpRecord::new(ShareOp::Move, &user(), 1, "教材", vec!["a".to_string()])
                .result(OpResult::Running),
        )
        .await
        .expect("書けなかった");
        finish(&pool, id, OpResult::Cancelled, 3, 1024, None).await;
        let row: (String, i64, i64) = sqlx::query_as(
            "SELECT result, item_count, byte_count FROM share_operations WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("読めなかった");
        assert_eq!(row, ("cancelled".to_string(), 3, 1024));
    }
}
