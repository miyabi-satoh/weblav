//! 共有の中を見る口 (一覧・ダウンロード・縮小画像・フォルダーの選択)。
//!
//! ログインしたユーザーが、その場所で見られる (`ShareAccess::Read` 以上) ときだけ開ける。
//! 見られない共有は、無いものと同じ 404 にする。

use std::collections::HashMap;
use std::path::Path as FsPath;

use axum::Json;
use axum::extract::{Path, Query, Request, State};
use axum::http::HeaderMap;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::api::contents::{self, FolderEntry, ServedFile, Visibility};
use crate::api::fs::path_to_string;
use crate::api::hidden::{Hidden, with_hidden};
use crate::api::sort::SortOrder;
use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

use super::ShareAccess;
use super::paths::{Resolved, resolve_existing};

#[derive(Debug, Deserialize, IntoParams)]
struct ShareBrowseQuery {
    /// 共有からの相対パス。区切りは `/`。省略時 (空文字列) は共有そのもの。
    #[serde(default)]
    path: String,
    /// 並び順。`title` (既定、タイトル順) か `new` (新しい順)。知らない値は既定として扱う。
    #[serde(default)]
    sort: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
struct SharePathQuery {
    /// 共有からの相対パス。区切りは `/`。
    #[serde(default)]
    path: String,
}

/// 共有を指す最小限の情報。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShareRef {
    pub id: i64,
    pub name: String,
}

/// その場所を載せているコンテンツ (→ docs/ui.md「掲載中」)。
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListedAs {
    pub content_id: i64,
    pub title: String,
}

/// 共有の中の1行。フォルダーの中の行と同じ形に、載せているコンテンツを添える。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShareEntry {
    #[serde(flatten)]
    entry: FolderEntry,
    /// このフォルダーかファイルを載せているコンテンツ。載せていなければ `None`。
    listed_as: Option<ListedAs>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShareBrowseResponse {
    share: ShareRef,
    /// この場所でできること。
    access: ShareAccess,
    /// 共有からの相対パス。実体の綴りを `/` でつなぐ。共有そのものなら空。
    path: String,
    entries: Vec<ShareEntry>,
    /// この場所を載せているコンテンツ。
    listed_as: Option<ListedAs>,
    /// アップロードできる1ファイルの大きさの上限 (バイト)。上限が無ければ `None`。
    upload_max_bytes: Option<u64>,
    /// この場所をホームに載せられるか (まだ載せていない)。
    can_list_on_home: bool,
    /// 消したものを OS のごみ箱へ送れる場所か。偽なら、消すと元に戻せない。
    trash: bool,
}

/// 共有の中の一覧。
#[utoipa::path(
    get,
    path = "/shares/{id}/browse",
    params(("id" = i64, Path), ShareBrowseQuery),
    responses(
        (status = OK, body = ShareBrowseResponse, description = "共有の中の一覧"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "共有・場所が無い、見られない、フォルダーでない"),
        (status = 503, body = crate::error::ErrorResponse, description = "共有のある NAS やディスクにつながらない"),
    )
)]
async fn browse_share(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<ShareBrowseQuery>,
) -> Result<Json<ShareBrowseResponse>, AppError> {
    let resolved = resolve_readable(&state, &user, id, &query.path).await?;
    let order = SortOrder::from_query(query.sort.as_deref());
    let dir = resolved.canonical.clone();
    let rel = resolved.rel.clone();
    let trash_policy = state.share_trash.clone();
    let (entries, trash) = with_hidden(&state, move |hidden| {
        if !dir.is_dir() {
            return Err(AppError::NotFound);
        }
        let trash = trash_policy.available(&dir);
        let entries = contents::read_entries(&dir, &rel, hidden, None, order)?;
        let entries = entries
            .into_iter()
            .map(|entry| {
                let path = path_to_string(dir.join(&entry.name));
                (entry, path)
            })
            .collect::<Vec<_>>();
        Ok((entries, trash))
    })
    .await?;

    let listed = ListedPaths::load(&state.pool, &user).await?;
    let listed_as = listed.get(&path_to_string(resolved.canonical.clone())?);
    let entries = entries
        .into_iter()
        .map(|(entry, path)| ShareEntry {
            listed_as: path.ok().and_then(|path| listed.get(&path)),
            entry,
        })
        .collect();
    Ok(Json(ShareBrowseResponse {
        share: ShareRef {
            id: resolved.root_id,
            name: resolved.root_name,
        },
        access: resolved.access,
        path: resolved.rel,
        entries,
        can_list_on_home: listed_as.is_none(),
        listed_as,
        upload_max_bytes: state.max_upload_bytes,
        trash,
    }))
}

/// 見られる場所だけを解決する。見られなければ 404。
async fn resolve_readable(
    state: &AppState,
    user: &AuthUser,
    id: i64,
    rel: &str,
) -> Result<Resolved, AppError> {
    let resolved = resolve_existing(state, user, id, rel).await?;
    if resolved.access < ShareAccess::Read {
        return Err(AppError::NotFound);
    }
    Ok(resolved)
}

/// フォルダーやファイルを指して載せている場所と、そのコンテンツ。`user` の管理画面の一覧に出ないもの
/// (他人の「作成者のみ」) と、ホームに出ない「非表示」は数えない。同じ場所をいくつも載せていれば、
/// 先に作ったものを採る。「非表示」しか無い場所は、載せ直せる (同じ場所の登録は断らない)。
struct ListedPaths(HashMap<String, ListedAs>);

impl ListedPaths {
    async fn load(pool: &sqlx::SqlitePool, user: &AuthUser) -> Result<Self, AppError> {
        let rows = sqlx::query!(
            r#"SELECT id as "id!", title, path as "path!", visibility as "visibility: Visibility", created_by
               FROM contents WHERE type IN ('folder', 'shortcut') AND path IS NOT NULL ORDER BY id"#
        )
        .fetch_all(pool)
        .await?;
        let mut listed = HashMap::new();
        for row in rows {
            if row.visibility == Visibility::Hidden
                || !contents::can_see_in_admin_list(user, row.visibility, row.created_by)
            {
                continue;
            }
            listed.entry(row.path).or_insert(ListedAs {
                content_id: row.id,
                title: row.title,
            });
        }
        Ok(Self(listed))
    }

    fn get(&self, path: &str) -> Option<ListedAs> {
        self.0.get(path).cloned()
    }
}

/// 共有の中のファイルのダウンロード。ブラウザーが直接開く。
#[utoipa::path(
    get,
    path = "/shares/{id}/download",
    params(("id" = i64, Path), SharePathQuery),
    responses(
        (status = OK),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "共有・ファイルが無い、見られない"),
    )
)]
async fn download_share_file(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<SharePathQuery>,
    request: Request,
) -> Result<Response, AppError> {
    let file = share_file(&state, &user, id, &query.path).await?;
    Ok(file.download(request).await)
}

/// 共有の中の画像の縮小画像 (→ docs/ui.md「画像のプレビュー」)。
#[utoipa::path(
    get,
    path = "/shares/{id}/thumbnail",
    params(("id" = i64, Path), SharePathQuery),
    responses(
        (status = OK, description = "縮小画像 (JPEG か PNG)"),
        (status = 304, description = "ETag が一致した"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "見られない・存在しない、またはプレビューしない画像"),
    )
)]
async fn share_thumbnail(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<SharePathQuery>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let file = share_file(&state, &user, id, &query.path).await?;
    file.thumbnail(&state, &headers).await
}

async fn share_file(
    state: &AppState,
    user: &AuthUser,
    id: i64,
    rel: &str,
) -> Result<ServedFile, AppError> {
    let resolved = resolve_readable(state, user, id, rel).await?;
    ServedFile::from_file(resolved.canonical).await
}

/// フォルダーの選択に出す1行。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShareDirEntry {
    name: String,
    /// 共有からの相対パス。次の階層を開くときにそのまま `path` に渡す。
    path: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShareDirsResponse {
    share: ShareRef,
    /// 共有からの相対パス。共有そのものなら空。
    path: String,
    /// 1つ上の階層。共有そのものでは `None`。
    parent: Option<String>,
    /// この場所でできること。移動・コピーの先には `add` 以上が要る。
    access: ShareAccess,
    entries: Vec<ShareDirEntry>,
}

/// 共有の中のフォルダーだけの一覧。移動・コピーの先を選ぶのに使う。
#[utoipa::path(
    get,
    path = "/shares/{id}/dirs",
    params(("id" = i64, Path), SharePathQuery),
    responses(
        (status = OK, body = ShareDirsResponse, description = "フォルダーの一覧"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "共有・場所が無い、見られない、フォルダーでない"),
    )
)]
async fn list_share_dirs(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<SharePathQuery>,
) -> Result<Json<ShareDirsResponse>, AppError> {
    let resolved = resolve_readable(&state, &user, id, &query.path).await?;
    let dir = resolved.canonical.clone();
    let rel = resolved.rel.clone();
    let entries = with_hidden(&state, move |hidden| {
        if !dir.is_dir() {
            return Err(AppError::NotFound);
        }
        read_subdirs(&dir, &rel, hidden).map_err(AppError::from)
    })
    .await?;
    let parent =
        (!resolved.rel.is_empty()).then(|| crate::api::fs::parent_rel(&resolved.rel).to_string());
    Ok(Json(ShareDirsResponse {
        share: ShareRef {
            id: resolved.root_id,
            name: resolved.root_name,
        },
        path: resolved.rel,
        parent,
        access: resolved.access,
        entries,
    }))
}

/// `dir` 直下の、一覧に出すフォルダー。名前の順。
///
/// ブロッキングI/Oを行うため、呼び出し側は `spawn_blocking` の中で呼ぶこと。
fn read_subdirs(
    dir: &FsPath,
    dir_rel: &str,
    hidden: &Hidden,
) -> std::io::Result<Vec<ShareDirEntry>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let Some((name, file_type)) = hidden.visible_entry(&entry) else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let path = crate::api::fs::join_rel(dir_rel, &name);
        entries.push(ShareDirEntry { name, path });
    }
    entries.sort_by(|a, b| crate::api::sort::title_cmp(&a.name, &b.name));
    Ok(entries)
}

pub(super) fn router() -> OpenApiRouter<AppState> {
    // ブラウザーが直接開くルートだけ、エラーを画面へのリダイレクトに変える (→ `api::browser`)。
    let download_routes = OpenApiRouter::new()
        .routes(routes!(download_share_file))
        .layer(axum::middleware::from_fn(
            crate::api::browser::redirect_errors_for_browsers,
        ));
    OpenApiRouter::new()
        .routes(routes!(browse_share))
        .routes(routes!(share_thumbnail))
        .routes(routes!(list_share_dirs))
        .merge(download_routes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 「非表示」はホームに出ないので、載せているものに数えない。同じ場所に別の行があれば、そちらを採る。
    #[sqlx::test]
    async fn hidden_contents_do_not_count_as_listed(pool: sqlx::SqlitePool) {
        let insert = |path: &'static str, visibility: &'static str| {
            let pool = pool.clone();
            async move {
                sqlx::query_scalar::<_, i64>(
                    "INSERT INTO contents (type, title, path, visibility) VALUES ('folder', ?1, ?1, ?2) RETURNING id",
                )
                .bind(path)
                .bind(visibility)
                .fetch_one(&pool)
                .await
                .expect("フォルダーを作れなかった")
            }
        };
        insert("/srv/hidden", "hidden").await;
        insert("/srv/both", "hidden").await;
        let shown = insert("/srv/both", "authenticated").await;
        let user = AuthUser::for_test(1, crate::auth::Role::Admin);

        let listed = ListedPaths::load(&pool, &user).await.expect("読めなかった");

        assert!(listed.get("/srv/hidden").is_none());
        assert_eq!(
            listed.get("/srv/both").map(|row| row.content_id),
            Some(shown)
        );
    }
}
