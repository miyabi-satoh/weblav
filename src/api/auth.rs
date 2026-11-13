use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use tower_sessions::Session;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::{self, AuthUser, Role, SESSION_USER_ID_KEY};
use crate::error::{AppError, AppJson, run_blocking};
use crate::state::AppState;

use super::operations::{Op, OpRecord, record_or_warn};
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

/// ログイン中の利用者。ほかの応答の `UserResponse` に、画面の出し分けに使う件数を添える。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MeResponse {
    #[serde(flatten)]
    pub user: UserResponse,
    /// 非表示のコンテンツの一覧 (`GET /contents/hidden`) に並ぶ件数。ユーザーメニューに出す。
    pub hidden_content_count: i64,
    /// リカバリコードを作った時刻。コードが無いときは `None`。
    /// アカウントのページで、控えの作成日時と見比べられるよう出す。
    pub recovery_code_created_at: Option<String>,
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

    let authed = auth::authenticate(&state.pool, &payload.username, &payload.password).await?;
    let Some(auth::Authenticated { user, credential }) = authed else {
        let err = AppError::InvalidCredentials;
        record_failed_sign_in(&state, Op::Login, &payload.username, &err).await;
        return Err(err);
    };

    // 認証に成功した試行はレートリミットの対象から外す。これが無いと、正しい
    // パスワードで短時間に何度もログインし直しただけの正当な利用者もロックされる。
    state.login_rate_limiter.release(&payload.username);

    start_session(&session, user.id, &credential).await?;
    record_or_warn(&state.pool, &OpRecord::new(Op::Login, &user)).await;

    Ok(Json(user.into()))
}

/// ログインとリカバリコードでの再設定の失敗を残す。他人のアカウントで試されていないかを、後から確かめるため。
/// 打ち込まれた名前のユーザーがいれば、その人の操作として絞り込めるよう id も残す。
/// ログインと同じく大文字と小文字を区別せずに探す (`auth::authenticate`)。区別すると、
/// 大文字と小文字だけ違う名前で試されたものが、その人で絞り込んでも出ない。
async fn record_failed_sign_in(state: &AppState, op: Op, username: &str, err: &AppError) {
    let user_id = match sqlx::query_scalar!(
        r#"SELECT id as "id!" FROM users WHERE username = ? COLLATE NOCASE"#,
        username
    )
    .fetch_optional(&state.pool)
    .await
    {
        Ok(id) => id,
        Err(err) => {
            tracing::warn!(error = %crate::error::error_chain(&err), "looking up a user for the operation log failed");
            None
        }
    };
    record_or_warn(
        &state.pool,
        &OpRecord::by(op, user_id, username).failed(err),
    )
    .await;
}

/// `user_id` でログインした状態にする。ログイン・初回セットアップ・リカバリコードでの戻しで共有する。
/// `credential` は、入れたパスワードの指紋 (`auth::credential_fingerprint`)。
///
/// セッション固定化攻撃対策: ログインのたびにセッションIDを振り直す。
pub(super) async fn start_session(
    session: &Session,
    user_id: i64,
    credential: &str,
) -> Result<(), AppError> {
    session.cycle_id().await.map_err(auth::Error::Session)?;
    session
        .insert(SESSION_USER_ID_KEY, user_id)
        .await
        .map_err(auth::Error::Session)?;
    // パスワード (かリカバリコード) を入れたばかりなので、確かめ直したことにする
    // (→ docs/access.md「操作の前のパスワードの確認」)。
    auth::mark_password_confirmed(session, credential).await
}

#[utoipa::path(post, path = "/auth/logout", responses((status = 204)))]
async fn logout(
    session: Session,
    viewer: auth::Viewer,
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    session.flush().await.map_err(auth::Error::Session)?;
    // ログインしていない人のログアウトは、何も変わらないので残さない。
    if let auth::Viewer::User(user) = viewer {
        record_or_warn(&state.pool, &OpRecord::new(Op::Logout, &user)).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/auth/me",
    responses(
        (status = OK, body = MeResponse),
        (status = 401, body = crate::error::ErrorResponse),
    )
)]
async fn me(user: AuthUser, State(state): State<AppState>) -> Result<Json<MeResponse>, AppError> {
    let hidden_content_count = super::contents::hidden_content_count(&state.pool, &user).await?;
    let recovery_code_created_at = sqlx::query_scalar!(
        "SELECT recovery_code_created_at FROM users WHERE id = ?",
        user.id
    )
    .fetch_optional(&state.pool)
    .await?
    .flatten();
    Ok(Json(MeResponse {
        user: user.into(),
        hidden_content_count,
        recovery_code_created_at,
    }))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ConfirmPasswordRequest {
    password: String,
}

/// 今のパスワードを確かめ直す (→ docs/access.md「操作の前のパスワードの確認」)。通ると、
/// 確かめ直しが要る操作を、しばらく聞き直さずに通す。
#[utoipa::path(
    post,
    path = "/auth/confirm-password",
    request_body = ConfirmPasswordRequest,
    responses(
        (status = 204, description = "確かめた"),
        (status = 401, description = "未ログイン、またはパスワードが違う", body = crate::error::ErrorResponse),
        (status = 429, description = "パスワードの試行が多すぎる", body = crate::error::ErrorResponse),
    )
)]
async fn confirm_password(
    user: AuthUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<ConfirmPasswordRequest>,
) -> Result<StatusCode, AppError> {
    verify_current_password(&state, &user, payload.password).await?;
    user.mark_confirmed().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ChangePasswordRequest {
    new_password: String,
}

/// 自分自身のパスワードを変更する。管理者による他人のパスワード再設定
/// (`PUT /admin/users/{id}/password`)と同じく、パスワードの確かめ直しを求める。
#[utoipa::path(
    put,
    path = "/auth/password",
    request_body = ChangePasswordRequest,
    responses(
        (status = 204, description = "変更した"),
        (status = 401, description = "未ログイン", body = crate::error::ErrorResponse),
        (status = 403, description = "パスワードの確かめ直しが要る", body = crate::error::ErrorResponse),
        (status = 422, description = "新しいパスワード未入力", body = crate::error::ErrorResponse),
    )
)]
async fn change_password(
    user: AuthUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<ChangePasswordRequest>,
) -> Result<StatusCode, AppError> {
    validate::non_empty(&payload.new_password, "new password is required")?;
    user.require_confirmation().await?;

    let new_hash = auth::hash_password_blocking(&payload.new_password).await?;
    sqlx::query!(
        "UPDATE users SET password_hash = ? WHERE id = ?",
        new_hash,
        user.id
    )
    .execute(&state.pool)
    .await?;
    record_or_warn(&state.pool, &OpRecord::new(Op::ChangePassword, &user)).await;
    // 新しいパスワードを決めたばかりなので、このセッションは確かめた内のままにする
    // (ほかのセッションは、古いパスワードで確かめた記録なので通らなくなる)。
    auth::mark_password_confirmed(
        &user.user_session()?.session,
        &auth::credential_fingerprint(&new_hash),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ChangeUsernameRequest {
    username: String,
}

/// 自分自身のユーザー名を変更する。パスワードの確かめ直しを求める (→ docs/access.md「ユーザー名の変更」)。
#[utoipa::path(
    put,
    path = "/auth/username",
    request_body = ChangeUsernameRequest,
    responses(
        (status = 204, description = "変更した"),
        (status = 401, description = "未ログイン", body = crate::error::ErrorResponse),
        (status = 403, description = "パスワードの確かめ直しが要る", body = crate::error::ErrorResponse),
        (status = 422, description = "新しいユーザー名が空、またはほかのユーザーが使っている", body = crate::error::ErrorResponse),
    )
)]
async fn change_username(
    user: AuthUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<ChangeUsernameRequest>,
) -> Result<StatusCode, AppError> {
    let username = validate::trimmed_non_empty(&payload.username, "username is required")?;
    user.require_confirmation().await?;

    auth::rename_user(&state.pool, user.id, &username).await?;
    if username != user.username {
        record_or_warn(
            &state.pool,
            &OpRecord::new(Op::ChangeUsername, &user)
                .target(user.id, &user.username)
                .renamed_to(&username),
        )
        .await;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// ログイン中の利用者の今のパスワードを確かめる。
async fn verify_current_password(
    state: &AppState,
    user: &AuthUser,
    current_password: String,
) -> Result<(), AppError> {
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

    let ok = run_blocking(move || auth::verify_password(&current_password, &row.password_hash))
        .await??;
    if !ok {
        return Err(AppError::IncorrectCurrentPassword);
    }
    state.login_rate_limiter.release(&user.username);
    Ok(())
}

/// 自分のリカバリコードを作り直す (→ docs/access.md「リカバリコード」)。前のものは無効になる。
/// パスワードの確かめ直しを求める。
#[utoipa::path(
    post,
    path = "/auth/recovery-code",
    responses(
        (status = OK, description = "作り直したリカバリコード", body = RecoveryCodeResponse),
        (status = 401, description = "未ログイン", body = crate::error::ErrorResponse),
        (status = 403, description = "パスワードの確かめ直しが要る", body = crate::error::ErrorResponse),
    )
)]
async fn create_recovery_code(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<RecoveryCodeResponse>, AppError> {
    user.require_confirmation().await?;

    let (recovery_code, hash) = auth::new_recovery_code().await?;
    sqlx::query!(
        "UPDATE users SET recovery_code_hash = ?, recovery_code_created_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
        hash,
        user.id
    )
    .execute(&state.pool)
    .await?;
    record_or_warn(&state.pool, &OpRecord::new(Op::CreateRecoveryCode, &user)).await;
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
        let err = AppError::InvalidRecoveryCode;
        record_failed_sign_in(&state, Op::Recover, &payload.username, &err).await;
        return Err(err);
    };
    state.recovery_rate_limiter.release(&payload.username);

    let password_hash = auth::hash_password_blocking(&payload.new_password).await?;
    let (recovery_code, recovery_hash) = auth::new_recovery_code().await?;
    // 照合に使ったコードのままの行だけ書く。同じコードで同時に2回戻されても、通るのは1回だけ。
    let user = sqlx::query!(
        r#"UPDATE users SET password_hash = ?, recovery_code_hash = ?,
                  recovery_code_created_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
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
    record_or_warn(
        &state.pool,
        &OpRecord::by(Op::Recover, Some(user_id), &user.username),
    )
    .await;

    start_session(
        &session,
        user_id,
        &auth::credential_fingerprint(&password_hash),
    )
    .await?;
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
        .routes(routes!(confirm_password))
        .routes(routes!(change_password))
        .routes(routes!(change_username))
        .routes(routes!(create_recovery_code))
        .routes(routes!(recover))
}
