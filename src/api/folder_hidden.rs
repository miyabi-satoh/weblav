//! フォルダー (ブラウズ型) の中で「表示しない」にしたファイル・サブフォルダー (→ docs/folders.md「表示しないもの」)。
//!
//! フォルダーは索引しないので、外したものだけを `folder_hidden_entries` に持つ。
//! 外したものは、一覧に出さないものと同じく、一覧・配信・縮小画像・検索のどれからも外す
//! (`contents::resolve_path` と `contents::read_entries`、検索は `folder_search`)。
//! 突き合わせは実体のパスで行う。綴りの大文字小文字を変えて開かれても外すため。

use std::path::{Path as FsPath, PathBuf};

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contents::{self, ContentType, Visibility};
use crate::auth::AuthUser;
use crate::error::{AppError, AppJson, run_blocking};
use crate::state::AppState;

/// 表示しないにしたものの、実体のパス。
pub(super) struct Hidden(Vec<PathBuf>);

impl Hidden {
    /// `path` (実体のパス) が、表示しないにしたものそのものか、その中か。
    pub(super) fn covers(&self, path: &FsPath) -> bool {
        self.0.iter().any(|hidden| path.starts_with(hidden))
    }
}

/// `content_id` のフォルダーで表示しないにしたものの、登録先からの相対パス。
pub(super) async fn rel_paths(pool: &SqlitePool, content_id: i64) -> Result<Vec<String>, AppError> {
    Ok(sqlx::query_scalar!(
        "SELECT path FROM folder_hidden_entries WHERE content_id = ?",
        content_id
    )
    .fetch_all(pool)
    .await?)
}

/// 相対パスを、`root` (登録先の実体のパス) の下の実体のパスにする。もう無いものは外す (外すものが無いため)。
///
/// ブロッキングI/Oを行うため、呼び出し側は `spawn_blocking` の中で呼ぶこと。
pub(super) fn resolve(root: &FsPath, rel_paths: &[String]) -> Hidden {
    Hidden(
        rel_paths
            .iter()
            .filter_map(|rel| std::fs::canonicalize(root.join(rel)).ok())
            .collect(),
    )
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FolderHiddenEntry {
    id: i64,
    /// フォルダーの登録先からの相対パス ('/' 区切り)。
    path: String,
    is_dir: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct HideFolderEntryRequest {
    /// 閲覧の画面と同じ、フォルダーの登録先からの相対パス。
    path: String,
}

/// `user` が書き換えられるフォルダーの登録先。フォルダーでなければ 404、書き換えられなければ 403 か 404。
async fn editable_folder(pool: &SqlitePool, user: &AuthUser, id: i64) -> Result<String, AppError> {
    let row = sqlx::query!(
        r#"SELECT path, visibility as "visibility: Visibility", created_by
           FROM contents WHERE id = ? AND type = ?"#,
        id,
        ContentType::Folder
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    contents::ensure_editable(user, row.visibility, row.created_by)?;
    contents::folder_path(row.path)
}

/// 表示しないにしたものの一覧。編集ページで取り消すために並べる。
#[utoipa::path(
    get,
    path = "/contents/{id}/hidden-entries",
    params(("id" = i64, Path)),
    responses(
        (status = OK, body = Vec<FolderHiddenEntry>, description = "表示しないにした順の新しいものから"),
        (status = 403, body = crate::error::ErrorResponse, description = "書き換えられるのは管理者と作成者だけ"),
        (status = 404, body = crate::error::ErrorResponse, description = "見えない・存在しない、またはフォルダーでない"),
    )
)]
async fn list_hidden_entries(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<FolderHiddenEntry>>, AppError> {
    editable_folder(&state.pool, &user, id).await?;
    Ok(Json(
        sqlx::query_as!(
            FolderHiddenEntry,
            r#"SELECT id as "id!", path, is_dir as "is_dir: bool"
               FROM folder_hidden_entries WHERE content_id = ? ORDER BY id DESC"#,
            id
        )
        .fetch_all(&state.pool)
        .await?,
    ))
}

/// フォルダーの中のファイルかサブフォルダーを表示しないにする。サブフォルダーなら中ごと外す。
#[utoipa::path(
    post,
    path = "/contents/{id}/hidden-entries",
    params(("id" = i64, Path)),
    request_body = HideFolderEntryRequest,
    responses(
        (status = OK, body = FolderHiddenEntry),
        (status = 403, body = crate::error::ErrorResponse, description = "書き換えられるのは管理者と作成者だけ"),
        (status = 404, body = crate::error::ErrorResponse, description = "見えない・存在しない、フォルダーでない、またはフォルダーの中に無いパス"),
        (status = 503, body = crate::error::ErrorResponse, description = "フォルダーのある NAS やディスクにつながらない"),
    )
)]
async fn hide_entry(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<HideFolderEntryRequest>,
) -> Result<Json<FolderHiddenEntry>, AppError> {
    let root = editable_folder(&state.pool, &user, id).await?;
    // 登録先そのものは外せない (それならコンテンツを消す)。
    if payload.path.is_empty() {
        return Err(AppError::NotFound);
    }
    // 表示しないにしたものの中も、見えないので外せない (`Some(id)`)。
    let target = contents::resolve_path(&state, &root, &payload.path, Some(id)).await?;
    let (path, is_dir) = run_blocking(move || {
        let root = std::fs::canonicalize(&root).map_err(|_| AppError::NotFound)?;
        let rel = target.strip_prefix(&root).map_err(|_| AppError::NotFound)?;
        let path = rel
            .components()
            .map(|component| component.as_os_str().to_str().map(str::to_string))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| AppError::Validation("path is not valid UTF-8".to_string()))?
            .join("/");
        Ok::<_, AppError>((path, target.is_dir()))
    })
    .await??;
    let entry = sqlx::query_as!(
        FolderHiddenEntry,
        r#"INSERT INTO folder_hidden_entries (content_id, path, is_dir, created_at) VALUES (?, ?, ?, unixepoch())
           ON CONFLICT (content_id, path) DO UPDATE SET is_dir = excluded.is_dir
           RETURNING id as "id!", path, is_dir as "is_dir: bool""#,
        id,
        path,
        is_dir
    )
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(entry))
}

/// 表示しないにしたものを取り消す。
#[utoipa::path(
    delete,
    path = "/contents/{id}/hidden-entries/{entry_id}",
    params(("id" = i64, Path), ("entry_id" = i64, Path)),
    responses(
        (status = 204),
        (status = 403, body = crate::error::ErrorResponse, description = "書き換えられるのは管理者と作成者だけ"),
        (status = 404, body = crate::error::ErrorResponse, description = "見えない・存在しない、またはこのフォルダーの表示しないものでない"),
    )
)]
async fn show_entry(
    user: AuthUser,
    State(state): State<AppState>,
    Path((id, entry_id)): Path<(i64, i64)>,
) -> Result<StatusCode, AppError> {
    editable_folder(&state.pool, &user, id).await?;
    let deleted = sqlx::query!(
        "DELETE FROM folder_hidden_entries WHERE id = ? AND content_id = ?",
        entry_id,
        id
    )
    .execute(&state.pool)
    .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_hidden_entries, hide_entry))
        .routes(routes!(show_entry))
}
