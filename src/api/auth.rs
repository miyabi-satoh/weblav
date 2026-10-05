use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use tower_sessions::Session;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::{self, AuthUser, Role, SESSION_USER_ID_KEY};
use crate::error::{AppError, AppJson};
use crate::state::AppState;

use super::validate;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserResponse {
    pub id: i64,
    pub username: String,
    pub role: Role,
    /// リカバリコードを作ってあるか (→ docs/access.md「リカバリコード」)。管理画面で作るよう促すのに使う。
    pub has_recovery_code: bool,
}

impl From<AuthUser> for UserResponse {
    fn from(user: AuthUser) -> Self {
        Self {
            id: user.id,
            username: user.username,
            role: user.role,
            has_recovery_code: user.has_recovery_code,
        }
    }
}

/// 利用者と、その人に作ったリカバリコード。初回セットアップとリカバリコードでの
/// 再設定が返す。コードはこの応答でしか返さない。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserWithRecoveryCodeResponse {
    pub user: UserResponse,
    pub recovery_code: String,
}

/// 作ったリカバリコード。見せる形 (5文字ずつ `-` で区切る)。この応答でしか返さない。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCodeResponse {
    pub recovery_code: String,
}

#[utoipa::path(
    post,
    path = "/auth/login",
    request_body = LoginRequest,
    responses(
        (status = OK, body = UserResponse),
        (status = 400, description = "リクエストボディが不正(JSONとして解釈できない等)", body = crate::error::ErrorResponse),
        (status = 401, body = crate::error::ErrorResponse),
        (status = 415, description = "Content-Typeがapplication/jsonでない", body = crate::error::ErrorResponse),
        (status = 422, description = "リクエストボディに必須フィールドが無い等", body = crate::error::ErrorResponse),
        (status = 429, body = crate::error::ErrorResponse),
    )
)]
async fn login(
    session: Session,
    State(state): State<AppState>,
    AppJson(payload): AppJson<LoginRequest>,
) -> Result<Json<UserResponse>, AppError> {
    // Argon2の検証はCPUバウンドなので、レートリミットは実際の検証(`authenticate`)より前に
    // 掛ける: 制限を超えた試行にはCPUコストを一切払わせない。
    if !state.login_rate_limiter.try_acquire(&payload.username) {
        return Err(AppError::TooManyRequests);
    }

    let user = auth::authenticate(&state.pool, &payload.username, &payload.password).await?;
    let Some(user) = user else {
        return Err(AppError::InvalidCredentials);
    };

    // 認証に成功した試行はレートリミットの対象から外す。これが無いと、正しい
    // パスワードで短時間に何度もログインし直しただけの正当な利用者もロックされる。
    state.login_rate_limiter.release(&payload.username);

    start_session(&session, user.id).await?;

    Ok(Json(user.into()))
}

/// `user_id` でログインした状態にする。ログインと初回セットアップで共有する。
///
/// セッション固定化攻撃対策: ログインのたびにセッションIDを振り直す。
pub(super) async fn start_session(session: &Session, user_id: i64) -> Result<(), AppError> {
    session.cycle_id().await.map_err(auth::Error::Session)?;
    session
        .insert(SESSION_USER_ID_KEY, user_id)
        .await
        .map_err(auth::Error::Session)?;
    Ok(())
}

#[utoipa::path(post, path = "/auth/logout", responses((status = 204)))]
async fn logout(session: Session) -> Result<StatusCode, AppError> {
    session.flush().await.map_err(auth::Error::Session)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/auth/me",
    responses(
        (status = OK, body = UserResponse),
        (status = 401, body = crate::error::ErrorResponse),
    )
)]
async fn me(user: AuthUser) -> Json<UserResponse> {
    Json(user.into())
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

/// 自分自身のパスワードを変更する。管理者による他人のパスワード再設定
/// (`PUT /admin/users/{id}/password`)とは異なり、現在のパスワードの入力を要求する。
#[utoipa::path(
    put,
    path = "/auth/password",
    request_body = ChangePasswordRequest,
    responses(
        (status = 204, description = "変更した"),
        (status = 401, description = "未ログイン、または現在のパスワードが違う", body = crate::error::ErrorResponse),
        (status = 409, description = "検証後に他の変更(管理者によるリセット等)と競合した", body = crate::error::ErrorResponse),
        (status = 422, description = "新しいパスワード未入力", body = crate::error::ErrorResponse),
        (status = 429, body = crate::error::ErrorResponse),
    )
)]
async fn change_password(
    user: AuthUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<ChangePasswordRequest>,
) -> Result<StatusCode, AppError> {
    validate::non_empty(&payload.new_password, "new password is required")?;

    let current_hash = verify_current_password(&state, &user, payload.current_password).await?;

    let new_hash = auth::hash_password_blocking(&payload.new_password).await?;
    // 検証に使ったハッシュと今のハッシュが一致する行だけ更新する。検証から更新までの
    // 間に管理者のリセット等で変わっていたら、その変更を黙って上書きしないよう競合として扱う。
    let updated = sqlx::query!(
        "UPDATE users SET password_hash = ? WHERE id = ? AND password_hash = ?",
        new_hash,
        user.id,
        current_hash
    )
    .execute(&state.pool)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "password was changed concurrently".to_string(),
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ChangeUsernameRequest {
    current_password: String,
    username: String,
}

/// 自分自身のユーザー名を変更する。パスワードの変更と同じく、今のパスワードを求める
/// (→ docs/access.md「ユーザー名の変更」)。
#[utoipa::path(
    put,
    path = "/auth/username",
    request_body = ChangeUsernameRequest,
    responses(
        (status = 204, description = "変更した"),
        (status = 401, description = "未ログイン、または現在のパスワードが違う", body = crate::error::ErrorResponse),
        (status = 422, description = "新しいユーザー名が空、またはほかのユーザーが使っている", body = crate::error::ErrorResponse),
        (status = 429, description = "今のパスワードの試行が多すぎる", body = crate::error::ErrorResponse),
    )
)]
async fn change_username(
    user: AuthUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<ChangeUsernameRequest>,
) -> Result<StatusCode, AppError> {
    let username = validate::trimmed_non_empty(&payload.username, "username is required")?;

    verify_current_password(&state, &user, payload.current_password).await?;

    auth::rename_user(&state.pool, user.id, &username).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// ログイン中の利用者の今のパスワードを確かめ、照合に使ったハッシュを返す
/// (パスワードの変更とリカバリコードの作り直しで共有する)。
async fn verify_current_password(
    state: &AppState,
    user: &AuthUser,
    current_password: String,
) -> Result<String, AppError> {
    // ログイン中でも、乗っ取ったセッションからの現在パスワードの総当たりを防ぐため
    // ログインと同じレートリミッターを流用する(ユーザー名単位のキーが同じなので、
    // ログイン試行とこのAPIの試行は同じ枠を共有する)。
    if !state.login_rate_limiter.try_acquire(&user.username) {
        return Err(AppError::TooManyRequests);
    }

    // `AuthUser` はDBから引き直した最新の情報だが、パスワードハッシュは持たない
    // (→ crate::auth::AuthUser)。検証のためだけに再取得する。
    let row = sqlx::query!("SELECT password_hash FROM users WHERE id = ?", user.id)
        .fetch_optional(&state.pool)
        .await?;
    let Some(row) = row else {
        // 検証の直前にアカウントが削除された場合。
        return Err(AppError::Unauthorized);
    };
    let current_hash = row.password_hash;

    let hash_to_verify = current_hash.clone();
    let ok = tokio::task::spawn_blocking(move || {
        auth::verify_password(&current_password, &hash_to_verify)
    })
    .await
    .map_err(auth::Error::Join)??;
    if !ok {
        return Err(AppError::IncorrectCurrentPassword);
    }
    state.login_rate_limiter.release(&user.username);
    Ok(current_hash)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct CreateRecoveryCodeRequest {
    current_password: String,
}

/// 自分のリカバリコードを作り直す (→ docs/access.md「リカバリコード」)。前のものは無効になる。
/// パスワードの変更と同じく、今のパスワードを求める。
#[utoipa::path(
    post,
    path = "/auth/recovery-code",
    request_body = CreateRecoveryCodeRequest,
    responses(
        (status = OK, description = "作り直したリカバリコード", body = RecoveryCodeResponse),
        (status = 401, description = "未ログイン、または現在のパスワードが違う", body = crate::error::ErrorResponse),
        (status = 409, description = "確かめてから書くまでにパスワードが変わった", body = crate::error::ErrorResponse),
        (status = 429, description = "今のパスワードの試行が多すぎる", body = crate::error::ErrorResponse),
    )
)]
async fn create_recovery_code(
    user: AuthUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<CreateRecoveryCodeRequest>,
) -> Result<Json<RecoveryCodeResponse>, AppError> {
    let current_hash = verify_current_password(&state, &user, payload.current_password).await?;

    let (recovery_code, hash) = auth::new_recovery_code().await?;
    // パスワードの変更と同じく、確かめたハッシュのままの行だけ書く。確かめてから書くまでに
    // 管理者がパスワードを再設定 (コードも消す) していたら、古いパスワードで作ったコードを
    // 書き戻さないよう競合として扱う。
    let updated = sqlx::query!(
        "UPDATE users SET recovery_code_hash = ? WHERE id = ? AND password_hash = ?",
        hash,
        user.id,
        current_hash
    )
    .execute(&state.pool)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "password was changed concurrently".to_string(),
        ));
    }
    Ok(Json(RecoveryCodeResponse { recovery_code }))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct RecoverRequest {
    username: String,
    recovery_code: String,
    new_password: String,
}

/// リカバリコードで利用者のパスワードを再設定し、ログインさせる (→ docs/access.md「リカバリコード」)。
/// コードは使い切り、新しいものを作って返す。
#[utoipa::path(
    post,
    path = "/auth/recover",
    request_body = RecoverRequest,
    responses(
        (status = OK, description = "再設定した利用者 (このセッションでログイン済み) と新しいリカバリコード", body = UserWithRecoveryCodeResponse),
        (status = 401, description = "ユーザー名かリカバリコードが違う (理由は区別しない)", body = crate::error::ErrorResponse),
        (status = 409, description = "照合の後に、ほかの操作でコードが変わった", body = crate::error::ErrorResponse),
        (status = 422, description = "新しいパスワード未入力", body = crate::error::ErrorResponse),
        (status = 429, description = "リカバリコードの試行が多すぎる", body = crate::error::ErrorResponse),
    )
)]
async fn recover(
    session: Session,
    State(state): State<AppState>,
    AppJson(payload): AppJson<RecoverRequest>,
) -> Result<Json<UserWithRecoveryCodeResponse>, AppError> {
    validate::non_empty(&payload.new_password, "new password is required")?;

    // ログインと同じく、Argon2 の照合より前に絞る。ログインとは別の枠にして、
    // こちらを叩かれても管理者がログインできなくならないようにする。
    if !state.recovery_rate_limiter.try_acquire(&payload.username) {
        return Err(AppError::TooManyRequests);
    }
    let verified =
        auth::verify_recovery_code(&state.pool, &payload.username, &payload.recovery_code).await?;
    let Some((user_id, used_hash)) = verified else {
        return Err(AppError::InvalidRecoveryCode);
    };
    state.recovery_rate_limiter.release(&payload.username);

    let password_hash = auth::hash_password_blocking(&payload.new_password).await?;
    let (recovery_code, recovery_hash) = auth::new_recovery_code().await?;
    // 照合に使ったコードのままの行だけ書く。同じコードで同時に2回戻されても、通るのは1回だけ。
    let user = sqlx::query!(
        r#"UPDATE users SET password_hash = ?, recovery_code_hash = ?
           WHERE id = ? AND recovery_code_hash = ?
           RETURNING username, role as "role: Role""#,
        password_hash,
        recovery_hash,
        user_id,
        used_hash
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::Conflict("recovery code was changed concurrently".to_string()))?;

    start_session(&session, user_id).await?;
    Ok(Json(UserWithRecoveryCodeResponse {
        user: UserResponse {
            id: user_id,
            username: user.username,
            role: user.role,
            has_recovery_code: true,
        },
        recovery_code,
    }))
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    // `routes!(a, b, c)` は「同一パスに複数メソッド」をまとめる書き方で、
    // パスの異なるハンドラを1回の呼び出しに詰めると内部でメソッドが衝突するため、
    // パスごとに`.routes()`を分けて呼ぶ。
    OpenApiRouter::new()
        .routes(routes!(login))
        .routes(routes!(logout))
        .routes(routes!(me))
        .routes(routes!(change_password))
        .routes(routes!(change_username))
        .routes(routes!(create_recovery_code))
        .routes(routes!(recover))
}
