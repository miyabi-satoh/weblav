//! 「共有」(→ docs/folders.md「共有」) の管理 API。
//!
//! コンテンツに使えるパスは、ここに登録したフォルダーの配下だけになる。
//! 追加できるのは**サーバーの PC の前にいる管理者**だけ (`LocalRequest` + `ConfirmedAdmin`)。
//! LAN の端末からは、追加の口があること自体を見せずに 404 にする。
//! 場所の選び直しも、窓で選ぶので同じく PC の前の管理者だけ。
//! 一覧・名前の変更・権限・削除は、LAN の端末の管理者もできる (一覧は `AdminUser`、ほかは `ConfirmedAdmin`。
//! → docs/access.md「操作の前のパスワードの確認」)。

use std::collections::HashMap;
use std::path::{Path as FsPath, PathBuf};
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::{AdminUser, ConfirmedAdmin};
use crate::error::{AppError, AppJson, run_blocking};
use crate::folder_reach::{OTHERS_WAIT, Reach, TARGET_WAIT};
use crate::state::AppState;

use super::error_detail::ValidationDetail;
use super::fs::{canonical_dir, inside_own_dirs, path_to_string};
use super::local::LocalRequest;
use super::operations::{FieldChange, Op, OpRecord, record_or_warn, recorded_username};
use super::shares::{AccessMap, ShareAccess};

/// 登録済みの「共有」1件。
///
/// 判定 (`is_within_roots`) には `path` だけを使う。`name` はコンテンツの登録側の画面で
/// 起点のフルパスの代わりに出す (→ docs/folders.md「共有」)。
#[derive(Debug, Clone)]
pub(super) struct Root {
    pub(super) id: i64,
    pub(super) name: String,
    pub(super) path: PathBuf,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RootResponse {
    pub id: i64,
    /// 登録側の画面でフルパスの代わりに出す名前。同じ名前は無い (大文字小文字も区別しない)。
    pub name: String,
    /// canonicalize 済みの絶対パス。
    pub path: String,
    /// このフォルダーの中にある登録済みコンテンツの数。
    pub content_count: usize,
    /// このフォルダーを外すと見られなくなるコンテンツの数。削除の確認に出す (→ docs/folders.md「共有」)。
    /// 登録中のほかの共有にも含まれるもの (入れ子) は数えない。
    pub removed_content_count: usize,
    /// 今読めるか。一覧でだけ確かめ、登録の応答は読めたものなので `ready`。
    pub status: RootStatus,
    /// 一般ユーザーがこの中でできることの既定 (→ `api::shares`)。
    pub editor_access: ShareAccess,
    /// ユーザーごとの例外。ユーザー名の順。
    pub user_access: Vec<RootUserAccess>,
    /// 除外のパターン (1行に1つ。共有の一番上から見る。→ `api::hidden`)。無ければ空文字列。
    pub exclude_patterns: String,
}

/// 共有の、ユーザーごとの例外の1件。
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RootUserAccess {
    pub user_id: i64,
    pub username: String,
    pub access: ShareAccess,
}

/// 共有の今の様子 (→ `folder_reach::Reach`)。
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum RootStatus {
    Ready,
    /// フォルダーが無い・読めない。
    Missing,
    /// NAS や外付けのディスクにつながらない。
    Unreachable,
}

impl From<&Reach> for RootStatus {
    fn from(reach: &Reach) -> Self {
        match reach {
            Reach::Ready(_) => Self::Ready,
            Reach::Gone => Self::Missing,
            Reach::Unreachable => Self::Unreachable,
        }
    }
}

#[utoipa::path(
    get,
    path = "/admin/roots",
    responses(
        (status = OK, body = Vec<RootResponse>, description = "登録済みの一覧"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
    )
)]
async fn list_roots(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<RootResponse>>, AppError> {
    let rows = sqlx::query!(
        r#"SELECT id as "id!", name, path, editor_access as "editor_access: ShareAccess", exclude_patterns
           FROM roots WHERE deleted_at IS NULL ORDER BY name COLLATE NOCASE"#
    )
    .fetch_all(&state.pool)
    .await?;
    let counts = ContentCounts::load(&state.pool).await?;
    let mut user_access = load_user_access(&state.pool).await?;
    // 管理者が様子を見る画面なので、眠っている NAS が起きるまで待つ。
    let paths = rows.iter().map(|row| row.path.clone()).collect();
    let reaches = check_all(&state, paths, TARGET_WAIT).await;
    let roots = rows
        .into_iter()
        .zip(reaches)
        .map(|(row, reach)| RootResponse {
            content_count: counts.under(&row.path),
            removed_content_count: counts.removed_with(&row.path),
            status: RootStatus::from(&reach),
            id: row.id,
            name: row.name,
            path: row.path,
            editor_access: row.editor_access,
            user_access: user_access.remove(&row.id).unwrap_or_default(),
            exclude_patterns: row.exclude_patterns.unwrap_or_default(),
        })
        .collect();
    Ok(Json(roots))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct CreateRootRequest {
    /// 絶対パス。フォルダー選択の窓 (`/admin/roots/pick`) が返す値をそのまま渡す。
    path: String,
    /// 名前。前後の空白は除く。省くか空ならフォルダー名にする。
    #[serde(default)]
    name: Option<String>,
}

#[utoipa::path(
    post,
    path = "/admin/roots",
    request_body = CreateRootRequest,
    responses(
        (status = 201, body = RootResponse, description = "登録したフォルダー"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外・パスワードの確かめ直しが要る"),
        (status = 404, body = crate::error::ErrorResponse, description = "サーバーのPC以外からの要求"),
        (status = 422, body = crate::error::ErrorResponse, description = "パスが存在しない・ディレクトリでない・weblav 自身の置き場の中・既存と重なる・同じ名前がある"),
    )
)]
async fn create_root(
    _: LocalRequest,
    admin: ConfirmedAdmin,
    State(state): State<AppState>,
    AppJson(payload): AppJson<CreateRootRequest>,
) -> Result<(StatusCode, Json<RootResponse>), AppError> {
    let path = registrable_path(&state, payload.path).await?;
    // Windows の canonicalize は `\\?\` を付けて返し、その形ではドライブや共有のルートに
    // フォルダー名が無いと判定されるので、落とした後の形から求める。
    let folder_name = default_name(FsPath::new(&path));
    let requested_name = super::validate::trimmed_or_none(payload.name.as_deref());

    let mut tx = crate::db::begin_write(&state.pool).await?;
    ensure_path_available(&mut tx, &path, None).await?;
    // 名前を指定したときだけ、重なりを拒む。フォルダー名で付ける名前は連番で避ける。
    let name = match requested_name {
        Some(name) => {
            ensure_name_available(&mut tx, name, None).await?;
            name.to_string()
        }
        None => free_name(&mut tx, &folder_name, None).await?,
    };

    // 削除済みの同じパスがあれば、その行を戻す (`path` は UNIQUE)。
    let row = sqlx::query!(
        r#"INSERT INTO roots (name, path) VALUES (?, ?)
           ON CONFLICT (path) DO UPDATE SET name = excluded.name, deleted_at = NULL
           RETURNING id as "id!", name, path, editor_access as "editor_access: ShareAccess",
                     exclude_patterns as "exclude_patterns?""#,
        name,
        path
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    record_or_warn(
        &state.pool,
        &OpRecord::new(Op::CreateShare, &admin).target(row.id, &row.name),
    )
    .await;
    // 0件とは限らない。削除したフォルダーを登録し直すと、その中のコンテンツが戻る
    // (コンテンツの登録は削除時に消していない)。
    let counts = ContentCounts::load(&state.pool).await?;
    // 登録し直した共有では、前の権限と除外の設定が残っている。
    let user_access = load_user_access(&state.pool)
        .await?
        .remove(&row.id)
        .unwrap_or_default();
    Ok((
        StatusCode::CREATED,
        Json(RootResponse {
            id: row.id,
            name: row.name,
            content_count: counts.under(&row.path),
            removed_content_count: counts.removed_with(&row.path),
            // 今 canonicalize できたフォルダー。
            status: RootStatus::Ready,
            path: row.path,
            editor_access: row.editor_access,
            user_access,
            exclude_patterns: row.exclude_patterns.unwrap_or_default(),
        }),
    ))
}

/// 窓で選ばれたパスを、共有として持てる実体のパスにする。登録と場所の選び直しで同じ検証を掛ける
/// (存在しない・ディレクトリでない・WebLAV 自身の置き場の中は 422)。
async fn registrable_path(state: &AppState, requested: String) -> Result<String, AppError> {
    ensure_reachable(state, &requested).await?;
    let own_dirs = state.own_dirs.clone();
    run_blocking(move || {
        let canonical = canonical_dir(&requested)?;
        if OwnDirs::resolve(&own_dirs).contains(&canonical) {
            return Err(inside_own_dirs());
        }
        path_to_string(canonical)
    })
    .await?
}

/// 同じフォルダーを2つの共有にしない。入れ子は許す (→ docs/folders.md「共有」)。削除済みは数えない。
/// `except` は場所を選び直す共有自身。
///
/// 確かめと書き込みは同じ書き込みトランザクションで行う。別々にすると、2つの要求が
/// 同時に来たときに、互いを見ないまま両方が確かめを通る。
async fn ensure_path_available(
    tx: &mut sqlx::SqliteConnection,
    path: &str,
    except: Option<i64>,
) -> Result<(), AppError> {
    let existing = sqlx::query_scalar!(
        "SELECT name FROM roots WHERE path = ? AND deleted_at IS NULL AND id IS NOT ?",
        path,
        except
    )
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(name) = existing {
        return Err(AppError::ValidationDetailed {
            message: "path is already a root".to_string(),
            detail: ValidationDetail::RootAlreadyRegistered { name },
        });
    }
    Ok(())
}

/// `base` が他の行と重なるとき、`base (1)` のように連番を付けた最初の空き。
/// 名前を省いて自動で付けるときに使う。大文字小文字は Unicode 全体で同一視する
/// (`ensure_name_available` の NOCASE は ASCII だけ)。区別しにくい名前を避けるための連番なので、
/// 広く同一視して困ることはない。
async fn free_name(
    tx: &mut sqlx::SqliteConnection,
    base: &str,
    except: Option<i64>,
) -> Result<String, AppError> {
    let names = sqlx::query_scalar!(
        "SELECT name FROM roots WHERE deleted_at IS NULL AND id IS NOT ?",
        except
    )
    .fetch_all(&mut *tx)
    .await?;
    let taken: std::collections::HashSet<String> =
        names.iter().map(|name| name.to_lowercase()).collect();
    Ok(super::validate::first_free_name(base, |candidate| {
        taken.contains(&candidate.to_lowercase())
    }))
}

/// 名前が、削除済みを除く他の行と重ならないか。`except` は名前を変える行自身。
/// インデックスと同じ NOCASE で比べる。
async fn ensure_name_available(
    tx: &mut sqlx::SqliteConnection,
    name: &str,
    except: Option<i64>,
) -> Result<(), AppError> {
    let taken = sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM roots
               WHERE name = ? COLLATE NOCASE AND deleted_at IS NULL AND id IS NOT ?
           ) as "taken!: bool""#,
        name,
        except
    )
    .fetch_one(&mut *tx)
    .await?;
    if taken {
        return Err(AppError::ValidationDetailed {
            message: "root name is already taken".to_string(),
            detail: ValidationDetail::RootNameTaken {
                name: name.to_string(),
            },
        });
    }
    Ok(())
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct RenameRootRequest {
    /// 新しい名前。前後の空白は除く。空ならフォルダー名にする。
    name: String,
}

#[utoipa::path(
    put,
    path = "/admin/roots/{id}",
    params(("id" = i64, Path)),
    request_body = RenameRootRequest,
    responses(
        (status = OK, body = RootResponse, description = "名前を変えたフォルダー"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外・パスワードの確かめ直しが要る"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しない"),
        (status = 422, body = crate::error::ErrorResponse, description = "同じ名前がある"),
    )
)]
async fn rename_root(
    admin: ConfirmedAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<RenameRootRequest>,
) -> Result<Json<RootResponse>, AppError> {
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let before = sqlx::query!(
        "SELECT path, name FROM roots WHERE id = ? AND deleted_at IS NULL",
        id
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;
    let path = before.path;
    let name = match super::validate::trimmed_or_none(Some(&payload.name)) {
        Some(name) => {
            ensure_name_available(&mut tx, name, Some(id)).await?;
            name.to_string()
        }
        None => free_name(&mut tx, &default_name(FsPath::new(&path)), Some(id)).await?,
    };
    sqlx::query!("UPDATE roots SET name = ? WHERE id = ?", name, id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    if before.name != name {
        record_or_warn(
            &state.pool,
            &OpRecord::new(Op::RenameShare, &admin)
                .target(id, &before.name)
                .renamed_to(&name),
        )
        .await;
    }

    root_response(&state, id).await.map(Json)
}

/// 1件ぶんの応答を、DB の今の値から組み立てる。名前や権限を変えた後に返す。
async fn root_response(state: &AppState, id: i64) -> Result<RootResponse, AppError> {
    let row = sqlx::query!(
        r#"SELECT name, path, editor_access as "editor_access: ShareAccess", exclude_patterns
           FROM roots WHERE id = ? AND deleted_at IS NULL"#,
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let counts = ContentCounts::load(&state.pool).await?;
    let user_access = load_user_access(&state.pool)
        .await?
        .remove(&id)
        .unwrap_or_default();
    // 画面は変えたあと一覧を読み直すので、ここでは待たない。
    let reach = state
        .folder_reach
        .check(&state.pool, &row.path, OTHERS_WAIT)
        .await;
    Ok(RootResponse {
        id,
        name: row.name,
        content_count: counts.under(&row.path),
        removed_content_count: counts.removed_with(&row.path),
        status: RootStatus::from(&reach),
        path: row.path,
        editor_access: row.editor_access,
        user_access,
        exclude_patterns: row.exclude_patterns.unwrap_or_default(),
    })
}

/// 共有ごとの、ユーザーごとの例外。ユーザー名の順。
async fn load_user_access(
    pool: &sqlx::SqlitePool,
) -> Result<HashMap<i64, Vec<RootUserAccess>>, AppError> {
    let rows = sqlx::query!(
        r#"SELECT root_user_access.root_id, root_user_access.user_id,
                  users.username, root_user_access.access as "access: ShareAccess"
           FROM root_user_access JOIN users ON users.id = root_user_access.user_id
           ORDER BY users.username COLLATE NOCASE"#
    )
    .fetch_all(pool)
    .await?;
    let mut by_root: HashMap<i64, Vec<RootUserAccess>> = HashMap::new();
    for row in rows {
        by_root
            .entry(row.root_id)
            .or_default()
            .push(RootUserAccess {
                user_id: row.user_id,
                username: row.username,
                access: row.access,
            });
    }
    Ok(by_root)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateRootAccessRequest {
    /// 一般ユーザーの既定。
    editor_access: ShareAccess,
    /// ユーザーごとの例外。ここに無いユーザーの例外は消す (送った内容で置き換える)。
    users: Vec<UserAccessRequest>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct UserAccessRequest {
    user_id: i64,
    access: ShareAccess,
}

/// 共有の中で一般ユーザーができることを変える (→ `api::shares`)。管理者は常にすべての操作ができるので、
/// 管理者の例外を送っても効かない。
#[utoipa::path(
    put,
    path = "/admin/roots/{id}/access",
    params(("id" = i64, Path)),
    request_body = UpdateRootAccessRequest,
    responses(
        (status = OK, body = RootResponse, description = "権限を変えた共有"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外・パスワードの確かめ直しが要る"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しない"),
        (status = 422, body = crate::error::ErrorResponse, description = "存在しないユーザーを含む"),
    )
)]
async fn update_root_access(
    admin: ConfirmedAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<UpdateRootAccessRequest>,
) -> Result<Json<RootResponse>, AppError> {
    // ユーザーの削除と行き違うと、消えたユーザーの例外を足そうとして外部キーで落ちるので、同じ鍵で並べる。
    let _users_guard = state.users_write_lock.lock().await;
    let mut tx = crate::db::begin_write(&state.pool).await?;
    // 同じ内容で保存したものは記録に残さないため、前の設定を取っておく。
    let before = sqlx::query!(
        r#"SELECT name, editor_access as "editor_access: ShareAccess" FROM roots
           WHERE id = ? AND deleted_at IS NULL"#,
        id
    )
    .fetch_optional(&mut *tx)
    .await?;
    let before_users: std::collections::BTreeMap<i64, ShareAccess> = sqlx::query!(
        r#"SELECT user_id, access as "access: ShareAccess" FROM root_user_access WHERE root_id = ?"#,
        id
    )
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|row| (row.user_id, row.access))
    .collect();
    let updated = sqlx::query!(
        "UPDATE roots SET editor_access = ? WHERE id = ? AND deleted_at IS NULL",
        payload.editor_access,
        id
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if updated == 0 {
        return Err(AppError::NotFound);
    }
    sqlx::query!("DELETE FROM root_user_access WHERE root_id = ?", id)
        .execute(&mut *tx)
        .await?;
    for user in &payload.users {
        let exists = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM users WHERE id = ?) as "exists!: bool""#,
            user.user_id
        )
        .fetch_one(&mut *tx)
        .await?;
        if !exists {
            return Err(AppError::Validation("unknown user".to_string()));
        }
        // 同じユーザーが2度来たら、後の方にする。
        sqlx::query!(
            "INSERT INTO root_user_access (root_id, user_id, access) VALUES (?, ?, ?)
             ON CONFLICT (root_id, user_id) DO UPDATE SET access = excluded.access",
            id,
            user.user_id,
            user.access
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    // 同じユーザーが2度来たら後の方にするのは、書き込みと同じ。
    let after_users: std::collections::BTreeMap<i64, ShareAccess> = payload
        .users
        .iter()
        .map(|user| (user.user_id, user.access))
        .collect();
    // 書き込みが済んだら、応答を組み立てる前に残す (組み立てに失敗しても、変えたことは残す)。
    // 行が無ければ上の UPDATE が 404 を返しているので、ここでは必ずある。
    if let Some(before) = before {
        let mut fields = Vec::new();
        if before.editor_access != payload.editor_access {
            fields.push(FieldChange::changed(
                "editorAccess",
                before.editor_access,
                payload.editor_access,
            ));
        }
        // 例外は人ごとに、前と後を残す。例外の無いほうは `null` (一般ユーザーの既定に従う)。
        let user_ids: std::collections::BTreeSet<i64> = before_users
            .keys()
            .chain(after_users.keys())
            .copied()
            .collect();
        for user_id in user_ids {
            let (old, new) = (before_users.get(&user_id), after_users.get(&user_id));
            if old == new {
                continue;
            }
            let username = recorded_username(&state.pool, user_id)
                .await
                .unwrap_or_default();
            fields.push(FieldChange::changed("userAccess", old, new).about(&username));
        }
        if !fields.is_empty() {
            record_or_warn(
                &state.pool,
                &OpRecord::new(Op::ChangeShareAccess, &admin)
                    .target(id, &before.name)
                    .fields(fields),
            )
            .await;
        }
    }
    root_response(&state, id).await.map(Json)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateRootExcludeRequest {
    /// 1行に1つ。空なら何も外さない。
    exclude_patterns: String,
}

/// 共有の除外のパターンを変える (→ `api::hidden`)。外すと見える範囲が広がるので、共有の追加と同じく
/// サーバーの PC の前の管理者だけ (→ docs/folders.md「登録と削除」)。
#[utoipa::path(
    put,
    path = "/admin/roots/{id}/exclude",
    params(("id" = i64, Path)),
    request_body = UpdateRootExcludeRequest,
    responses(
        (status = OK, body = RootResponse, description = "除外を変えた共有"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外・パスワードの確かめ直しが要る"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しない・サーバーの PC の外から"),
        (status = 422, body = crate::error::ErrorResponse, description = "パターンが正しくない"),
    )
)]
async fn update_root_exclude(
    _: LocalRequest,
    admin: ConfirmedAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<UpdateRootExcludeRequest>,
) -> Result<Json<RootResponse>, AppError> {
    let patterns = super::file_filter::normalize_patterns(Some(&payload.exclude_patterns));
    super::hidden::share_filter(patterns.as_deref())?;
    let before = sqlx::query!(
        "SELECT name, path, exclude_patterns FROM roots WHERE id = ? AND deleted_at IS NULL",
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    sqlx::query!(
        "UPDATE roots SET exclude_patterns = ? WHERE id = ?",
        patterns,
        id
    )
    .execute(&state.pool)
    .await?;
    if before.exclude_patterns != patterns {
        record_or_warn(
            &state.pool,
            &OpRecord::new(Op::ChangeShareExclude, &admin)
                .target(id, &before.name)
                .fields(vec![FieldChange::named("excludePatterns")]),
        )
        .await;
        super::hidden::apply_to_indexes(&state, Some(&before.path)).await;
    }
    root_response(&state, id).await.map(Json)
}

#[utoipa::path(
    delete,
    path = "/admin/roots/{id}",
    params(("id" = i64, Path)),
    responses(
        (status = 204, description = "削除した"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外・パスワードの確かめ直しが要る"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しない"),
    )
)]
async fn delete_root(
    admin: ConfirmedAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    // 行は消さずに削除済みの印を付ける。残ったコンテンツの場所を、起点を隠したまま出すため
    // (→ docs/folders.md「共有」)。
    let name = sqlx::query_scalar!(
        "UPDATE roots SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND deleted_at IS NULL RETURNING name",
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    record_or_warn(
        &state.pool,
        &OpRecord::new(Op::DeleteShare, &admin).target(id, &name),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// 登録済みの「共有」を、判定に使える形 (canonicalize 済み) で返す。
///
/// 判定のたびに読む。件数は多くても数件で、キャッシュを持つと登録・削除の直後に
/// 古い判定が残る (→ docs/folders.md「共有」)。
///
/// **実体を辿れないものは落とす**。登録を消さずにフォルダーだけ消された場合、その中は配らない。
/// つながらない NAS やディスクのものも落とす。ほかのフォルダーのために待たないよう、
/// 確かめは短く打ち切る (`OTHERS_WAIT`)。開こうとしているフォルダーは、先に
/// `ensure_reachable` で待っておくこと。
/// canonicalize しておくのは、判定の相手 (`fs::canonical_dir` の戻り値) と同じ形で
/// 比べるため。保存してある文字列は Windows の長いパスで `\\?\` の有無が食い違いうる
/// (→ `fs::simplify_verbatim`)。
pub(super) async fn load_roots(state: &AppState) -> Result<Vec<Root>, AppError> {
    let rows = sqlx::query!(
        r#"SELECT id as "id!", name, path FROM roots WHERE deleted_at IS NULL ORDER BY name COLLATE NOCASE"#
    )
    .fetch_all(&state.pool)
    .await?;
    let paths = rows.iter().map(|row| row.path.clone()).collect();
    let reaches = check_all(state, paths, OTHERS_WAIT).await;
    Ok(rows
        .into_iter()
        .zip(reaches)
        .filter_map(|(row, reach)| match reach {
            Reach::Ready(path) => Some(Root {
                id: row.id,
                name: row.name,
                path,
            }),
            Reach::Gone | Reach::Unreachable => None,
        })
        .collect())
}

/// `load_roots` のうち、`user` が中を見られるもの (→ `api::shares`)。登録先を選ぶ口
/// (ディレクトリの一覧・数え上げ・フォルダーの登録先) に使う。
///
/// 見られない共有を外すだけでよい。入れ子で外側が見られれば、内側の場所は外側に含まれるので
/// 判定 (`is_within_roots`) を通る。これは共有の中でできることを、含む共有のうち一番強いもので決めるのと同じ。
pub(super) async fn load_roots_for(
    state: &AppState,
    user: &crate::auth::AuthUser,
) -> Result<Vec<Root>, AppError> {
    let roots = load_roots(state).await?;
    let access = AccessMap::load(&state.pool, user).await?;
    Ok(roots
        .into_iter()
        .filter(|root| access.of_root(root.id) >= ShareAccess::Read)
        .collect())
}

/// フォルダーの様子を並べて確かめる。並びは `paths` のまま。
pub(super) async fn check_all(
    state: &AppState,
    paths: Vec<String>,
    wait: Duration,
) -> Vec<Reach> {
    let mut checks = tokio::task::JoinSet::new();
    for (index, path) in paths.into_iter().enumerate() {
        let reach = state.folder_reach.clone();
        let pool = state.pool.clone();
        checks.spawn(async move { (index, reach.check(&pool, &path, wait).await) });
    }
    let mut reaches: Vec<(usize, Reach)> = checks.join_all().await;
    reaches.sort_by_key(|(index, _)| *index);
    reaches.into_iter().map(|(_, reach)| reach).collect()
}

/// `path` を含む登録中の共有が、つながらない NAS やディスクにあれば
/// `FolderUnreachable`。開こうとしているフォルダーなので、眠っている NAS が起きるまで待つ
/// (`TARGET_WAIT`)。フォルダーが無いときは通す (それぞれの呼び出し側が今までどおり 404 などにする)。
pub(super) async fn ensure_reachable(state: &AppState, path: &str) -> Result<(), AppError> {
    if reachable_all(state, &[path], TARGET_WAIT).await?[0] {
        Ok(())
    } else {
        Err(AppError::FolderUnreachable)
    }
}

/// `ensure_reachable` の、エラーにしない形。検索のように、つながらないフォルダーを飛ばして続けるときに使う。
/// 並びは `paths` のまま。
///
/// 確かめるのは、それぞれのパスを含む登録中の共有。どれにも含まれない
/// (登録から外した後のコンテンツ・綴りの違う入力など) なら、そのパス自身を確かめる。
/// 呼び出し側は、この後でそのパスを canonicalize するので、ここを素通りさせると NAS が落ちているときに止まる。
pub(super) async fn reachable_all(
    state: &AppState,
    paths: &[&str],
    wait: Duration,
) -> Result<Vec<bool>, AppError> {
    let roots = sqlx::query_scalar!("SELECT path FROM roots WHERE deleted_at IS NULL")
        .fetch_all(&state.pool)
        .await?;
    let targets: Vec<Vec<String>> = paths
        .iter()
        .map(|path| {
            let containing: Vec<String> = roots
                .iter()
                .filter(|root| contains(root, path))
                .cloned()
                .collect();
            if containing.is_empty() {
                vec![path.to_string()]
            } else {
                containing
            }
        })
        .collect();
    let mut unique: Vec<String> = targets.iter().flatten().cloned().collect();
    unique.sort();
    unique.dedup();
    let reaches = check_all(state, unique.clone(), wait).await;
    let unreachable: Vec<&String> = unique
        .iter()
        .zip(&reaches)
        .filter(|(_, reach)| **reach == Reach::Unreachable)
        .map(|(path, _)| path)
        .collect();
    Ok(targets
        .iter()
        .map(|targets| !targets.iter().any(|target| unreachable.contains(&target)))
        .collect())
}

/// 登録側の画面に出す場所。起点のフルパスの代わりに、共有の名前と
/// そこから先のパスを返す (→ docs/folders.md「共有」)。
#[derive(Debug, PartialEq, Eq)]
pub(super) struct RootLocation {
    pub(super) root_name: String,
    /// 共有から先の相対パス。区切りは `/` にそろえる。フォルダーそのものなら空。
    pub(super) path_in_root: String,
    /// 含む共有が削除済みか。
    pub(super) root_deleted: bool,
}

/// 場所の表示に使う、保存されている登録 (canonicalize しない)。判定には使わない。
#[derive(Debug, Clone)]
pub(super) struct RootRecord {
    pub(super) name: String,
    pub(super) path: String,
    pub(super) deleted: bool,
}

/// 削除済みも含めた登録。ファイルシステムに触らないので、フォルダーが消えた登録でも名前を出せる。
pub(super) async fn load_root_records(
    pool: &sqlx::SqlitePool,
) -> Result<Vec<RootRecord>, AppError> {
    Ok(
        sqlx::query!(r#"SELECT name, path, deleted_at IS NOT NULL as "deleted!: bool" FROM roots"#)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|row| RootRecord {
                name: row.name,
                path: row.path,
                deleted: row.deleted,
            })
            .collect(),
    )
}

/// `path` (コンテンツに保存された canonicalize 済みの絶対パス) を含む共有を探し、
/// 画面に出す場所を組み立てる。どれにも含まれなければ `None`。
///
/// 登録中が当たれば、最も外側を採る (入れ子の起点の決まりは `containing_root` と同じ)。
/// 削除済みしか当たらなければ、最も内側を採る。
pub(super) fn locate(records: &[RootRecord], path: &str) -> Option<RootLocation> {
    let matching = || records.iter().filter(|record| contains(&record.path, path));
    let record = matching()
        .filter(|record| !record.deleted)
        .min_by_key(|record| record.path.len())
        .or_else(|| matching().max_by_key(|record| record.path.len()))?;
    let rest = FsPath::new(path)
        .strip_prefix(FsPath::new(&record.path))
        .ok()?;
    let path_in_root = rest
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    Some(RootLocation {
        root_name: record.name.clone(),
        path_in_root,
        root_deleted: record.deleted,
    })
}

/// 名前を省いたときの名前。フォルダー名で、ドライブのルートのように無ければパスそのもの。
///
/// **`path_to_string` を通した後のパスを渡すこと** (`\\?\` 付きの形では、ルートの判定が変わる)。
pub(super) fn default_name(path: &FsPath) -> String {
    super::fs::file_name_lossy(path).unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// `canonical` が、登録済みのどれかの中にあるか。
///
/// **パスを使う操作はすべてこれを通す** (→ docs/folders.md「共有」)。登録時の検証だけでは、
/// 登録後にフォルダーを削除された場合にそのまま配信され続ける。
///
/// 表示しないもの (→ `api::hidden`) の中も外す。WebLAV 自身の置き場 (設定とデータ) は、共有がその祖先
/// (`C:\` など) でも、登録先にも配信にも使わせない。`canonical` はフォルダーとして見る。
pub(super) fn is_within_roots(
    roots: &[Root],
    canonical: &FsPath,
    hidden: &super::hidden::Hidden,
) -> bool {
    !hidden.hides_path(canonical, true) && containing_root(roots, canonical).is_some()
}

/// `canonical` を含む登録済みのフォルダー。選択 UI のパンくずを、辿れるところから始めるのにも使う
/// (→ docs/folders.md「一覧 API」)。入れ子でいくつも当たれば、最も外側を採る。
/// 内側を採ると、外側から辿って内側へ入ったときに起点が切り替わり、「上へ」で外側へ戻れない。
///
/// **両方とも `std::fs::canonicalize` 済みのパスであること** (→ `load_roots`)。
pub(super) fn containing_root<'a>(roots: &'a [Root], canonical: &FsPath) -> Option<&'a Root> {
    roots
        .iter()
        .filter(|root| canonical.starts_with(&root.path))
        .min_by_key(|root| root.path.components().count())
}

/// weblav 自身の置き場 (設定とデータ) の実体パス。まだ作られていないものは除く
/// (中身が無いので、外すものも無い)。
///
/// どこを公開するかは設置者に任せるが、ここだけはアプリが必ず外す
/// (→ docs/folders.md「共有」)。守りたいのは選ぶことではなく中身が出ることなので、祖先は選べる。
///
/// 一覧や走査ではエントリごとに比べるので、要求ごとに `resolve` で1回だけ求めて持ち回る。
#[derive(Debug, Default)]
pub(super) struct OwnDirs(Vec<PathBuf>);

impl OwnDirs {
    /// ブロッキングI/Oを行うため、`spawn_blocking` の中で呼ぶこと。
    pub(super) fn resolve(dirs: &[PathBuf]) -> Self {
        Self(
            dirs.iter()
                .filter_map(|dir| std::fs::canonicalize(dir).ok())
                .collect(),
        )
    }

    /// `path` が、置き場のどれかそのものか、その中か。**実体パスを渡すこと**
    /// (canonicalize 済みか、canonicalize 済みの根からリンクを辿らずに得たもの)。
    pub(super) fn contains(&self, path: &FsPath) -> bool {
        self.0.iter().any(|dir| path.starts_with(dir))
    }

    /// 置き場のどれかが、`path` そのものかその中にあるか (`path` を動かすと置き場も動く)。**実体パスを渡すこと**。
    pub(super) fn any_within(&self, path: &FsPath) -> bool {
        self.0.iter().any(|dir| dir.starts_with(path))
    }
}

/// 共有ごとのコンテンツの件数を数えるための材料。
///
/// パスを持つコンテンツ (`folder`・`shortcut`) の登録先と、登録中の共有。
/// 多くても数百件なので、まとめて読んで数える。SQL で前方一致を書くと、区切り文字の扱いを
/// `contains` と二重に持つことになる。
struct ContentCounts {
    content_paths: Vec<String>,
    root_paths: Vec<String>,
}

impl ContentCounts {
    async fn load(pool: &sqlx::SqlitePool) -> Result<Self, AppError> {
        let content_paths = sqlx::query_scalar!("SELECT path FROM contents WHERE path IS NOT NULL")
            .fetch_all(pool)
            .await?
            .into_iter()
            .flatten()
            .collect();
        let root_paths = sqlx::query_scalar!("SELECT path FROM roots WHERE deleted_at IS NULL")
            .fetch_all(pool)
            .await?;
        Ok(Self {
            content_paths,
            root_paths,
        })
    }

    /// `root` の中にあるコンテンツの数。
    fn under(&self, root: &str) -> usize {
        self.content_paths
            .iter()
            .filter(|path| contains(root, path))
            .count()
    }

    /// `root` を外すと見られなくなるコンテンツの数。ほかの登録にも含まれるもの (入れ子) は数えない。
    fn removed_with(&self, root: &str) -> usize {
        self.content_paths
            .iter()
            .filter(|path| contains(root, path))
            .filter(|path| {
                !self
                    .root_paths
                    .iter()
                    .any(|other| other != root && contains(other, path))
            })
            .count()
    }
}

/// `parent` が `child` を含むか (同じ場所も含むとみなす)。
///
/// **どちらも `path_to_string` を通した値であること**。同じ場所は同じ文字列になる。
///
/// 文字列の前方一致ではなく `Path` の要素で比べる。`C:\教材` が `C:\教材2` を含むと
/// 判定されず、マルチバイトのパスでも文字の途中で切らない。大文字小文字も区別する
/// (canonicalize 済みなので、同じ場所なら同じ綴りになる)。
pub(super) fn contains(parent: &str, child: &str) -> bool {
    FsPath::new(child).starts_with(FsPath::new(parent))
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct PickRootResponse {
    /// 選ばれたフォルダーの絶対パス。キャンセルされたら `null`。登録はしていないので、
    /// 登録するには `POST /admin/roots` へそのまま渡す。
    path: Option<String>,
}

/// サーバーの PC に OS 標準のフォルダー選択の窓を出し、選ばれたパスを返す (→ docs/folders.md「選び方」)。
///
/// 窓は同時に1つだけ。要求した側が待つのをやめても、窓が閉じるまでは開いている扱いにする
/// (番は `run_blocking` の中で持つ。ハンドラが drop されても、ブロッキングの処理は最後まで走る)。
#[utoipa::path(
    post,
    path = "/admin/roots/pick",
    responses(
        (status = OK, body = PickRootResponse, description = "選ばれたパス。キャンセルなら null"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外・パスワードの確かめ直しが要る"),
        (status = 404, body = crate::error::ErrorResponse, description = "サーバーのPC以外からの要求"),
        (status = 409, body = crate::error::ErrorResponse, description = "窓がもう開いている"),
    )
)]
async fn pick_root(
    _: LocalRequest,
    _admin: ConfirmedAdmin,
    State(state): State<AppState>,
) -> Result<Json<PickRootResponse>, AppError> {
    let session = state
        .folder_picker
        .begin()
        .ok_or_else(|| AppError::Conflict("a folder picker is already open".to_string()))?;
    let picked = run_blocking(move || {
        session
            .pick()
            .map_err(|_| AppError::Io(std::io::Error::other("the folder picker is not running")))
    })
    .await??;
    let Some(picked) = picked else {
        return Ok(Json(PickRootResponse { path: None }));
    };
    // 画面に出し、そのまま登録に渡す値なので、ほかの口と同じ形に揃える。
    let path = path_to_string(picked)?;
    keep_bookmark(&state, path.clone()).await?;
    Ok(Json(PickRootResponse { path: Some(path) }))
}

/// 窓で選んだ今のうちに、次の起動でも読み書きできるようにしておく (→ docs/distribution.md「ビルド・配布の方法」)。
/// キーは登録と同じく実体のパス。macOS 以外では何もしない。
async fn keep_bookmark(state: &AppState, path: String) -> Result<(), AppError> {
    let bookmark = run_blocking(move || -> Result<_, AppError> {
        let Ok(canonical) = canonical_dir(&path) else {
            return Ok(None);
        };
        // 作れないまま登録させると、サンドボックスでは起動し直した後に読めないフォルダーが残る。
        let Some(bookmark) = crate::folder_access::bookmark(&canonical).map_err(|err| {
            AppError::Io(std::io::Error::other(format!(
                "failed to create a folder bookmark: {err}"
            )))
        })?
        else {
            return Ok(None);
        };
        Ok(Some((path_to_string(canonical)?, bookmark)))
    })
    .await??;
    if let Some((canonical, bookmark)) = bookmark {
        crate::folder_access::save(&state.pool, &canonical, &bookmark).await?;
    }
    Ok(())
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct MoveRootRequest {
    /// 新しい場所の絶対パス。フォルダー選択の窓 (`/admin/roots/pick`) が返す値をそのまま渡す。
    path: String,
}

/// 共有の場所を、窓で選び直したフォルダーに付け替える (→ docs/shares.md「共有の場所の選び直し」)。
///
/// フォルダーを別のディスクへ移した・ドライブの文字が変わったときに、共有を消して登録し直さずに済むようにする。
/// 権限と名前は共有の id に付いているので、そのまま残る。中のフォルダーのコンテンツの登録先は、
/// 新しい場所の同じ相対パスへ付け替える。
///
/// 今と同じフォルダーを選んだときは何も付け替えない (ブックマークは、窓で選んだときに作り直してある)。
#[utoipa::path(
    put,
    path = "/admin/roots/{id}/path",
    params(("id" = i64, Path)),
    request_body = MoveRootRequest,
    responses(
        (status = OK, body = RootResponse, description = "場所を付け替えた共有"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外・パスワードの確かめ直しが要る"),
        (status = 404, body = crate::error::ErrorResponse, description = "サーバーのPC以外からの要求か、共有が無い"),
        (status = 422, body = crate::error::ErrorResponse, description = "パスが存在しない・ディレクトリでない・weblav 自身の置き場の中・ほかの共有と同じ"),
        (status = 503, body = crate::error::ErrorResponse, description = "選んだ場所につながらない"),
    )
)]
async fn move_root(
    _: LocalRequest,
    admin: ConfirmedAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<MoveRootRequest>,
) -> Result<Json<RootResponse>, AppError> {
    sqlx::query_scalar!(
        "SELECT id FROM roots WHERE id = ? AND deleted_at IS NULL",
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let path = registrable_path(&state, payload.path).await?;

    let mut moved = true;
    let (name, list_views) = {
        // コンテンツの登録先の書き換えと直列化する (→ `AppState::contents_write_lock`)。
        let _write_guard = state.contents_write_lock.lock().await;
        let mut tx = crate::db::begin_write(&state.pool).await?;
        // 記録に写す名前も、同じトランザクションの中で読む (間に名前を変えられても、そのときの名前にする)。
        let current = sqlx::query!(
            "SELECT path, name FROM roots WHERE id = ? AND deleted_at IS NULL",
            id
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;
        let old = current.path;
        let list_views = if old == path {
            moved = false;
            Vec::new()
        } else {
            ensure_path_available(&mut tx, &path, Some(id)).await?;
            // 削除済みの共有が同じパスを持っていれば、その行を消す (`path` は UNIQUE)。その行は残ったコンテンツの
            // 場所を出すためだけのもので、そのコンテンツは付け替えた後の共有の中に入る。
            sqlx::query!(
                "DELETE FROM roots WHERE path = ? AND deleted_at IS NOT NULL",
                path
            )
            .execute(&mut *tx)
            .await?;
            sqlx::query!("UPDATE roots SET path = ? WHERE id = ?", path, id)
                .execute(&mut *tx)
                .await?;
            rebase_contents(&mut tx, FsPath::new(&old), FsPath::new(&path)).await?
        };
        tx.commit().await?;
        (current.name, list_views)
    };
    if moved {
        record_or_warn(
            &state.pool,
            &OpRecord::new(Op::MoveShare, &admin).target(id, &name),
        )
        .await;
    }

    // 窓で選んだときに作ったブックマーク (`pick_root`) で、新しい場所の許可を戻す。macOS 以外では何もしない。
    crate::folder_access::restore(&state.pool).await;
    // 中身が変わっていれば、一覧の表示の索引を次の再スキャンを待たずに読み直す (→ docs/archive.md「スキャン」)。
    super::shares::repoint::refresh_after_change(&state, list_views);
    root_response(&state, id).await.map(Json)
}

/// `old` の中に登録先があるフォルダーのコンテンツとショートカットを、`new` の同じ相対パスへ付け替える。
/// 共有ごと動かした先を選び直す操作で、中の並びは同じなので、どちらも同じ相対パスへ移せる。
/// 一覧の表示を使うコンテンツの id を返す (中を読み直させるため)。
///
/// 中の索引は登録先からの相対パスなので、そのまま使える。フォルダーごと動かした先を
/// 選び直す使い方なので、中の並びは同じとみなす。
async fn rebase_contents(
    conn: &mut sqlx::SqliteConnection,
    old: &FsPath,
    new: &FsPath,
) -> Result<Vec<i64>, AppError> {
    let rows = sqlx::query!(
        r#"SELECT id as "id!", path as "path!", list_view as "list_view: bool"
           FROM contents WHERE type IN ('folder', 'shortcut') AND path IS NOT NULL"#
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut list_views = Vec::new();
    for row in rows {
        if rebase_content_path(&mut *conn, row.id, FsPath::new(&row.path), old, new).await?
            && row.list_view
        {
            list_views.push(row.id);
        }
    }
    Ok(list_views)
}

/// コンテンツ `id` の登録先 `path` が `old` そのものかその中なら、`new` の同じ相対パスへ書き換える。書き換えたら `true`。
pub(in crate::api) async fn rebase_content_path(
    conn: &mut sqlx::SqliteConnection,
    id: i64,
    path: &FsPath,
    old: &FsPath,
    new: &FsPath,
) -> Result<bool, AppError> {
    let Some(path) = rebase(path, old, new) else {
        return Ok(false);
    };
    let path = path_to_string(path)?;
    sqlx::query!("UPDATE contents SET path = ? WHERE id = ?", path, id)
        .execute(&mut *conn)
        .await?;
    Ok(true)
}

/// `path` が `old` そのものかその中なら、`new` の同じ相対パス。そうでなければ `None`。
///
/// `old` そのものは `new` そのものにする。空の相対パスを `join` すると末尾に区切りが付き、
/// 保存してあるほかのパスと綴りが合わなくなるため。
pub(super) fn rebase(path: &FsPath, old: &FsPath, new: &FsPath) -> Option<PathBuf> {
    let rest = path.strip_prefix(old).ok()?;
    Some(if rest.as_os_str().is_empty() {
        new.to_path_buf()
    } else {
        new.join(rest)
    })
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_roots, create_root))
        .routes(routes!(pick_root))
        .routes(routes!(move_root))
        .routes(routes!(rename_root, delete_root))
        .routes(routes!(update_root_access))
        .routes(routes!(update_root_exclude))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::shares::repoint::tests::{content_path, insert_folder, sep_path};

    #[test]
    fn a_root_contains_its_descendants() {
        let sep = std::path::MAIN_SEPARATOR;
        let parent = format!("{sep}srv{sep}media");
        assert!(contains(&parent, &format!("{parent}{sep}2026")));
        assert!(contains(&parent, &parent));
    }

    /// 区切り文字まで見ないと、名前の前方一致で別のフォルダーを含むと誤判定する。
    #[test]
    fn a_sibling_with_a_longer_name_is_not_contained() {
        let sep = std::path::MAIN_SEPARATOR;
        let parent = format!("{sep}srv{sep}media");
        assert!(!contains(&parent, &format!("{sep}srv{sep}media2")));
    }

    /// 名前がマルチバイトで、隣のフォルダー名がその前方一致になる形。
    /// バイト単位で切ると文字の途中に当たって panic する。
    #[test]
    fn a_multibyte_sibling_is_not_contained() {
        let sep = std::path::MAIN_SEPARATOR;
        let parent = format!("{sep}srv{sep}教材");
        assert!(!contains(&parent, &format!("{sep}srv{sep}教材集")));
        assert!(contains(&parent, &format!("{sep}srv{sep}教材{sep}2026")));
    }

    /// 綴りの違うフォルダーを同じ場所とみなさない。Linux では別のディレクトリで、
    /// 大文字小文字を無視すると登録していない場所まで配ってしまう。
    #[test]
    fn a_differently_cased_path_is_not_contained() {
        let sep = std::path::MAIN_SEPARATOR;
        let parent = format!("{sep}srv{sep}media");
        assert!(!contains(&parent, &format!("{sep}srv{sep}MEDIA{sep}2026")));
    }

    #[test]
    fn a_location_is_the_root_name_and_the_rest() {
        let sep = std::path::MAIN_SEPARATOR;
        let root = format!("{sep}srv{sep}教材");
        let records = vec![record("教材", &root, false)];

        assert_eq!(
            locate(&records, &format!("{root}{sep}英検{sep}2024")),
            Some(RootLocation {
                root_name: "教材".to_string(),
                path_in_root: "英検/2024".to_string(),
                root_deleted: false,
            })
        );
        assert_eq!(
            locate(&records, &root),
            Some(RootLocation {
                root_name: "教材".to_string(),
                path_in_root: String::new(),
                root_deleted: false,
            })
        );
        // 前方一致だけの隣のフォルダーは含まない (`contains` と同じ)。
        assert_eq!(locate(&records, &format!("{root}集")), None);
    }

    fn record(name: &str, path: &str, deleted: bool) -> RootRecord {
        RootRecord {
            name: name.to_string(),
            path: path.to_string(),
            deleted,
        }
    }

    /// 登録中を削除済みより優先する。登録中どうしでは外側、削除済みどうしでは内側を採る。
    #[test]
    fn a_location_prefers_a_registered_root_then_the_innermost_deleted_one() {
        let sep = std::path::MAIN_SEPARATOR;
        let outer = format!("{sep}srv");
        let inner = format!("{sep}srv{sep}教材");
        let target = format!("{inner}{sep}英検");

        let deleted_only = vec![record("外", &outer, true), record("内", &inner, true)];
        let location = locate(&deleted_only, &target).expect("見つかるはず");
        assert_eq!(location.root_name, "内");
        assert!(location.root_deleted);

        let with_registered = vec![record("外", &outer, false), record("内", &inner, true)];
        let location = locate(&with_registered, &target).expect("見つかるはず");
        assert_eq!(location.root_name, "外");
        assert_eq!(location.path_in_root, "教材/英検");
        assert!(!location.root_deleted);

        let nested = vec![record("内", &inner, false), record("外", &outer, false)];
        let location = locate(&nested, &target).expect("見つかるはず");
        assert_eq!(location.root_name, "外");
    }

    #[test]
    fn the_default_name_is_the_folder_name() {
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(
            default_name(FsPath::new(&format!("{sep}srv{sep}教材"))),
            "教材"
        );
        let top = if cfg!(windows) { "C:\\" } else { "/" };
        assert_eq!(default_name(FsPath::new(top)), top);
    }

    /// 外すと見られなくなる件数は、ほかの登録にも含まれるもの (入れ子) を数えない。
    #[test]
    fn removed_count_leaves_out_contents_another_root_covers() {
        let sep = std::path::MAIN_SEPARATOR;
        let outer = format!("{sep}srv");
        let inner = format!("{outer}{sep}教材");
        let counts = ContentCounts {
            content_paths: vec![format!("{inner}{sep}英検"), format!("{outer}{sep}写真")],
            root_paths: vec![outer.clone(), inner.clone()],
        };
        assert_eq!(counts.under(&outer), 2);
        assert_eq!(counts.removed_with(&outer), 1);
        assert_eq!(counts.under(&inner), 1);
        assert_eq!(counts.removed_with(&inner), 0);
    }

    /// 入れ子では、最も外側を起点にする。
    #[test]
    fn the_containing_root_is_the_outermost() {
        let sep = std::path::MAIN_SEPARATOR;
        let root = |name: &str, path: &str| Root {
            id: 0,
            name: name.to_string(),
            path: PathBuf::from(path),
        };
        let outer = format!("{sep}srv");
        let inner = format!("{outer}{sep}教材");
        let roots = vec![root("外", &outer), root("内", &inner)];
        let found =
            |path: &str| containing_root(&roots, FsPath::new(path)).map(|r| r.name.as_str());
        assert_eq!(found(&format!("{inner}{sep}英検")), Some("外"));
        assert_eq!(found(&format!("{outer}{sep}写真")), Some("外"));
    }

    /// 共有の中の登録先は新しい場所の同じ相対パスへ移り、頭の似た隣のフォルダーは動かない。
    /// 一覧の表示を使うものだけを、読み直す相手として返す。
    #[sqlx::test]
    async fn rebasing_moves_contents_inside_the_old_place_only(pool: sqlx::SqlitePool) {
        let old = sep_path(&["srv", "教材"]);
        let new = sep_path(&["mnt", "nas", "教材"]);
        let top = insert_folder(&pool, &old, false).await;
        let inner = insert_folder(&pool, &old.join("英検"), true).await;
        let sibling = insert_folder(&pool, &sep_path(&["srv", "教材2"]), true).await;

        let mut conn = pool.acquire().await.expect("つなげなかった");
        let list_views = rebase_contents(&mut conn, &old, &new)
            .await
            .expect("付け替えられなかった");
        drop(conn);

        assert_eq!(list_views, vec![inner]);
        assert_eq!(content_path(&pool, top).await, new.display().to_string());
        assert_eq!(
            content_path(&pool, inner).await,
            new.join("英検").display().to_string()
        );
        assert_eq!(
            content_path(&pool, sibling).await,
            sep_path(&["srv", "教材2"]).display().to_string()
        );
    }

    /// 付け替えた登録先が変わると、一覧の表示の索引の版が上がる (配る側の使い回しを捨てさせる)。
    #[sqlx::test]
    async fn rebasing_bumps_the_index_version(pool: sqlx::SqlitePool) {
        let old = sep_path(&["srv", "教材"]);
        let id = insert_folder(&pool, &old.join("英検"), true).await;
        let version = |pool: sqlx::SqlitePool| async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT COALESCE((SELECT index_version FROM archive_versions WHERE content_id = ?), 0)",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("版を読めなかった")
        };
        let before = version(pool.clone()).await;

        let mut conn = pool.acquire().await.expect("つなげなかった");
        rebase_contents(&mut conn, &old, &sep_path(&["mnt", "教材"]))
            .await
            .expect("付け替えられなかった");
        drop(conn);

        assert!(version(pool.clone()).await > before);
    }

    /// ほかの共有と同じ場所は拒み、選び直す共有自身と削除済みの共有は数えない。
    #[sqlx::test]
    async fn a_place_another_root_holds_is_not_available(pool: sqlx::SqlitePool) {
        let insert = |name: &'static str, path: &'static str, deleted: bool| {
            let pool = pool.clone();
            async move {
                sqlx::query_scalar::<_, i64>(
                    "INSERT INTO roots (name, path, deleted_at) VALUES (?, ?, CASE WHEN ? THEN 'x' END) RETURNING id",
                )
                .bind(name)
                .bind(path)
                .bind(deleted)
                .fetch_one(&pool)
                .await
                .expect("共有を作れなかった")
            }
        };
        let own = insert("自分", "/srv/a", false).await;
        insert("ほか", "/srv/b", false).await;
        insert("消した", "/srv/c", true).await;
        let mut conn = pool.acquire().await.expect("つなげなかった");

        assert!(
            ensure_path_available(&mut conn, "/srv/a", Some(own))
                .await
                .is_ok()
        );
        assert!(
            ensure_path_available(&mut conn, "/srv/c", Some(own))
                .await
                .is_ok()
        );
        match ensure_path_available(&mut conn, "/srv/b", Some(own)).await {
            Err(AppError::ValidationDetailed {
                detail: ValidationDetail::RootAlreadyRegistered { name },
                ..
            }) => assert_eq!(name, "ほか"),
            other => panic!("ほかの共有の場所を通した: {other:?}"),
        }
    }

    #[test]
    fn rebasing_the_place_itself_adds_no_trailing_separator() {
        let old = sep_path(&["srv", "教材"]);
        let new = sep_path(&["mnt", "教材"]);
        assert_eq!(rebase(&old, &old, &new), Some(new.clone()));
        assert_eq!(rebase(&old.join("a"), &old, &new), Some(new.join("a")));
        assert_eq!(rebase(&sep_path(&["srv", "教材2"]), &old, &new), None);
    }

    /// 自身の置き場 (設定とデータ) そのものと中だけを外し、祖先 (`C:\` など) は外さない。
    #[test]
    fn only_own_dirs_and_inside_are_left_out() {
        let dir = crate::test_support::TempDir::new("roots-data-dir");
        let data_dir = dir.path().join("weblav");
        std::fs::create_dir_all(data_dir.join("logs")).expect("データ置き場を作れなかった");
        let data_dir = std::fs::canonicalize(&data_dir).expect("canonicalize できなかった");
        let base = std::fs::canonicalize(dir.path()).expect("canonicalize できなかった");

        let config_dir = base.join("config");
        std::fs::create_dir_all(&config_dir).expect("設定の置き場を作れなかった");
        let own_dirs = OwnDirs::resolve(&[config_dir.clone(), data_dir.clone()]);

        assert!(!own_dirs.contains(&base));
        assert!(own_dirs.contains(&data_dir));
        assert!(own_dirs.contains(&data_dir.join("logs")));
        assert!(own_dirs.contains(&config_dir));
        assert!(!own_dirs.contains(&base.join("other")));
    }
}
