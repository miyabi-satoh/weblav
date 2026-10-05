//! ユーザー管理API (→ docs/access.md「ロールと操作」)。全ハンドラ `AdminUser` 限定。

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::{self, AdminUser, Role};
use crate::error::{AppError, AppJson};
use crate::state::AppState;

use super::contents::Visibility;
use super::error_detail::ValidationDetail;
use super::free_limit;
use super::validate;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserListItem {
    pub id: i64,
    pub username: String,
    pub role: Role,
    pub created_at: String,
    /// そのユーザーが作成者の `private` の件数。削除の確認で、`hidden` として残る件数を
    /// 示すため (→ docs/access.md「ユーザーの削除と作成者」)。
    pub private_content_count: i64,
}

struct UserRow {
    id: i64,
    username: String,
    role: Role,
    created_at: String,
    private_content_count: i64,
}

impl From<UserRow> for UserListItem {
    fn from(row: UserRow) -> Self {
        Self {
            id: row.id,
            username: row.username,
            role: row.role,
            created_at: row.created_at,
            private_content_count: row.private_content_count,
        }
    }
}

async fn current_role(conn: &mut SqliteConnection, id: i64) -> Result<Role, AppError> {
    sqlx::query_scalar!(r#"SELECT role as "role: Role" FROM users WHERE id = ?"#, id)
        .fetch_optional(conn)
        .await?
        .ok_or(AppError::NotFound)
}

/// 変えた後の応答を一覧と同じ形にするため、件数の副問い合わせ込みで引き直す
/// (`RETURNING` には副問い合わせを書けない)。
async fn fetch_list_item(conn: &mut SqliteConnection, id: i64) -> Result<UserListItem, AppError> {
    let row = sqlx::query_as!(
        UserRow,
        r#"SELECT id as "id!", username, role as "role: Role", created_at,
                  (SELECT COUNT(*) FROM contents
                   WHERE contents.created_by = users.id AND contents.visibility = ?)
                      as "private_content_count!: i64"
           FROM users WHERE id = ?"#,
        Visibility::Private,
        id,
    )
    .fetch_one(conn)
    .await?;
    Ok(UserListItem::from(row))
}

#[utoipa::path(
    get,
    path = "/admin/users",
    responses(
        (status = OK, body = Vec<UserListItem>, description = "ユーザーの一覧"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
    )
)]
async fn list_users(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<UserListItem>>, AppError> {
    let rows = sqlx::query_as!(
        UserRow,
        r#"SELECT id as "id!", username, role as "role: Role", created_at,
                  (SELECT COUNT(*) FROM contents
                   WHERE contents.created_by = users.id AND contents.visibility = ?)
                      as "private_content_count!: i64"
           FROM users
           ORDER BY username COLLATE NOCASE"#,
        Visibility::Private,
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows.into_iter().map(UserListItem::from).collect()))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct CreateUserRequest {
    username: String,
    password: String,
    /// 権限。省略時は `user`。
    #[serde(default)]
    role: Option<Role>,
}

#[utoipa::path(
    post,
    path = "/admin/users",
    request_body = CreateUserRequest,
    responses(
        (status = 201, body = UserListItem, description = "作成したユーザー"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 409, body = crate::error::ErrorResponse, description = "Free の上限に当たった"),
        (status = 422, body = crate::error::ErrorResponse, description = "ユーザー名重複・パスワード未入力"),
    )
)]
async fn create_user(
    _admin: AdminUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserListItem>), AppError> {
    let username = validate::trimmed_non_empty(&payload.username, "username is required")?;
    // CLIの`--create-user`と同じ制約(空パスワードは拒否)。こちらはプロンプトの
    // 確認入力が無いため、二重入力の一致はフロント側の責務とする。
    validate::non_empty(&payload.password, "password is required")?;

    let role = payload.role.unwrap_or(Role::User);
    // 数えてから作るまでを直列化する (→ `super::free_limit`)。
    let _write_guard = state.users_write_lock.lock().await;
    free_limit::check_role(state.pro.edition(), &state.pool, role).await?;
    let created = auth::create_user(&state.pool, &username, &payload.password, role, None).await?;

    Ok((
        StatusCode::CREATED,
        Json(UserListItem {
            id: created.id,
            username,
            role,
            created_at: created.created_at,
            private_content_count: 0,
        }),
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateRoleRequest {
    role: Role,
}

/// `id`が最後の1人のadminで、かつ`admin`以外へ変えようとしている場合は拒否する
/// (→ docs/access.md「admin の最後の1人」)。呼び出し側のトランザクション内で使うこと
/// (カウントと更新の間に別リクエストの変更が割り込むと、admin不在の状態が
/// 一時的にでも生じ得るため)。
async fn reject_if_last_admin(
    tx: &mut sqlx::SqliteConnection,
    current_role: Role,
    new_role: Role,
) -> Result<(), AppError> {
    if current_role != Role::Admin || new_role == Role::Admin {
        return Ok(());
    }
    let admin_count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64" FROM users WHERE role = ?"#,
        Role::Admin
    )
    .fetch_one(tx)
    .await?;
    if admin_count <= 1 {
        return Err(AppError::ValidationDetailed {
            message: "cannot demote or delete the last admin".to_string(),
            detail: ValidationDetail::LastAdmin,
        });
    }
    Ok(())
}

#[utoipa::path(
    put,
    path = "/admin/users/{id}/role",
    params(("id" = i64, Path)),
    request_body = UpdateRoleRequest,
    responses(
        (status = OK, body = UserListItem, description = "更新後のユーザー"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しないユーザー"),
        (status = 409, body = crate::error::ErrorResponse, description = "Free の上限に当たった"),
        (status = 422, body = crate::error::ErrorResponse, description = "最後のadminを降格しようとした"),
    )
)]
async fn update_role(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<UpdateRoleRequest>,
) -> Result<Json<UserListItem>, AppError> {
    // カウントと更新の間を`users_write_lock`で直列化する
    // (`AppState::users_write_lock`のドキュメント参照)。
    let _write_guard = state.users_write_lock.lock().await;
    let mut tx = crate::db::begin_write(&state.pool).await?;

    let current_role = current_role(&mut tx, id).await?;

    reject_if_last_admin(&mut tx, current_role, payload.role).await?;
    if payload.role != current_role {
        free_limit::check_role(state.pro.edition(), &mut *tx, payload.role).await?;
    }

    sqlx::query!("UPDATE users SET role = ? WHERE id = ?", payload.role, id)
        .execute(&mut *tx)
        .await?;
    let item = fetch_list_item(&mut tx, id).await?;
    tx.commit().await?;

    Ok(Json(item))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ResetPasswordRequest {
    password: String,
}

#[utoipa::path(
    put,
    path = "/admin/users/{id}/password",
    params(("id" = i64, Path)),
    request_body = ResetPasswordRequest,
    responses(
        (status = 204, description = "変更した"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しないユーザー"),
        (status = 422, body = crate::error::ErrorResponse, description = "パスワード未入力"),
    )
)]
async fn reset_password(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<ResetPasswordRequest>,
) -> Result<StatusCode, AppError> {
    validate::non_empty(&payload.password, "password is required")?;

    let hash = auth::hash_password_blocking(&payload.password).await?;

    // パスワードを知られて再設定するときに、知った人が作ったコードで取り戻されないよう、コードも消す
    // (→ docs/access.md「リカバリコード」)。
    let updated = sqlx::query!(
        "UPDATE users SET password_hash = ?, recovery_code_hash = NULL WHERE id = ?",
        hash,
        id
    )
    .execute(&state.pool)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct RenameUserRequest {
    username: String,
}

/// ほかのユーザーのユーザー名を変える。パスワードの再設定と同じく、本人のパスワードは求めない。
#[utoipa::path(
    put,
    path = "/admin/users/{id}/username",
    params(("id" = i64, Path)),
    request_body = RenameUserRequest,
    responses(
        (status = OK, body = UserListItem, description = "変更したユーザー"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しないユーザー"),
        (status = 422, body = crate::error::ErrorResponse, description = "ユーザー名が空、またはほかのユーザーが使っている"),
    )
)]
async fn update_username(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<RenameUserRequest>,
) -> Result<Json<UserListItem>, AppError> {
    let username = validate::trimmed_non_empty(&payload.username, "username is required")?;

    let mut tx = crate::db::begin_write(&state.pool).await?;
    auth::rename_user(&mut *tx, id, &username).await?;
    let item = fetch_list_item(&mut tx, id).await?;
    tx.commit().await?;

    Ok(Json(item))
}

#[utoipa::path(
    delete,
    path = "/admin/users/{id}",
    params(("id" = i64, Path)),
    responses(
        (status = 204, description = "削除した"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しないユーザー"),
        (status = 422, body = crate::error::ErrorResponse, description = "最後のadminを削除しようとした"),
    )
)]
async fn delete_user(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    // update_roleと同じ理由で直列化する(`AppState::users_write_lock`のドキュメント参照)。
    // 作成者の記録を消すため`contents_write_lock`も取る。取る順は`users_write_lock`が先。
    let _users_guard = state.users_write_lock.lock().await;
    let _contents_guard = state.contents_write_lock.lock().await;
    let mut tx = crate::db::begin_write(&state.pool).await?;

    let current_role = current_role(&mut tx, id).await?;

    // 降格と同じ「admin不在」の不変条件を守るため、削除も`Role::User`への変更と
    // 同じ扱いでチェックする(→ docs/access.md「admin の最後の1人」)。
    reject_if_last_admin(&mut tx, current_role, Role::User).await?;

    // その人の`private`を`hidden`に変えてから作成者の記録を消す(→ docs/access.md「ユーザーの削除と作成者」)。
    sqlx::query!(
        r#"UPDATE contents SET visibility = ?,
               updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
           WHERE created_by = ? AND visibility = ?"#,
        Visibility::Hidden,
        id,
        Visibility::Private,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "UPDATE contents SET created_by = NULL WHERE created_by = ?",
        id
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!("DELETE FROM users WHERE id = ?", id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_users, create_user))
        .routes(routes!(update_role))
        .routes(routes!(reset_password))
        .routes(routes!(update_username))
        .routes(routes!(delete_user))
}
