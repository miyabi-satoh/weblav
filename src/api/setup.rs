//! 初回セットアップ (最初の管理者の作成、→ docs/access.md「初回セットアップ」)。
//!
//! どの口もこの PC の中からの要求だけを通し、それ以外には 404 を返す。
//! 断る理由 (セットアップ済み・トークンが違う・期限切れ・外からの要求) も 404 に揃える。
//! 応答の違いから「この PC にはまだ管理者がいない」と分かってしまわないようにするため。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use serde::Deserialize;
use tower_sessions::Session;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::{self, Role};
use crate::error::{AppError, AppJson};
use crate::state::AppState;

use super::auth::{UserResponse, UserWithRecoveryCodeResponse};
use super::backup::{StageBackupRequest, StagedBackupResponse};
use super::local::LocalRequest;
use super::validate;

/// ADR: 発行したトークンの寿命。タスクトレイから押してブラウザーで入力を終えるまでの時間で、
/// 迷っても足り、開いたまま放置された画面がいつまでも使えることもない長さにする。
const TOKEN_TTL: Duration = Duration::from_secs(10 * 60);

/// ADR: トークンの長さ (バイト)。総当たりが現実的でない 256bit。
const TOKEN_BYTES: usize = 32;

/// バックアップを受け取る口で、トークンを添えるヘッダ。
/// multipart の中に入れると、ファイルを受け取り終えるまでトークンを確かめられないため。
const TOKEN_HEADER: &str = "x-setup-token";

/// 発行済みのセットアップ用トークン。プロセスのメモリにだけ置き、ディスクにも DB にも
/// 書かない (→ docs/access.md「初回セットアップ」)。同時に有効なのは最後に発行した1つだけ。
///
/// `tokio::sync::Mutex` ではなく `std::sync::Mutex` を使う。保持するのは値を差し替える
/// 一瞬だけで、この鍵を持ったまま await しないため (`ArchiveScans` と同じ)。
#[derive(Default)]
pub struct SetupTokens {
    issued: Mutex<Option<IssuedToken>>,
}

struct IssuedToken {
    value: String,
    expires_at: Instant,
}

impl SetupTokens {
    /// 新しいトークンを発行する。前に発行したものはこの時点で無効になる。
    fn issue(&self) -> Result<String, AppError> {
        self.issue_with_ttl(TOKEN_TTL)
    }

    fn issue_with_ttl(&self, ttl: Duration) -> Result<String, AppError> {
        let value = super::random_hex::<TOKEN_BYTES>()?;
        let mut issued = self.lock();
        *issued = Some(IssuedToken {
            value: value.clone(),
            expires_at: Instant::now() + ttl,
        });
        Ok(value)
    }

    /// 有効なトークンかを確かめる。使い切りはしない (画面を出してよいかの判定に使う)。
    fn verify(&self, candidate: &str) -> bool {
        matches!(&*self.lock(), Some(token) if token.is_valid(candidate))
    }

    /// 有効なら使い切る。管理者を作れたときだけ呼ぶ。
    fn consume(&self, candidate: &str) {
        let mut issued = self.lock();
        if matches!(&*issued, Some(token) if token.is_valid(candidate)) {
            *issued = None;
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<IssuedToken>> {
        self.issued.lock().expect("setup token lock poisoned")
    }
}

impl IssuedToken {
    fn is_valid(&self, candidate: &str) -> bool {
        self.expires_at > Instant::now() && constant_time_eq(&self.value, candidate)
    }
}

/// 先頭から何文字合っていたかが応答の速さに出ないよう、最後まで見比べてから結果を返す。
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 管理者が1人もいなければ、セットアップはまだ済んでいない (→ docs/access.md「初回セットアップ」)。
async fn setup_required(state: &AppState) -> Result<bool, AppError> {
    let admin_count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64" FROM users WHERE role = ?"#,
        Role::Admin
    )
    .fetch_one(&state.pool)
    .await?;
    Ok(admin_count == 0)
}

/// セットアップ済みなら 404 にする。
async fn require_setup_required(state: &AppState) -> Result<(), AppError> {
    if setup_required(state).await? {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

async fn require_valid_setup_token(state: &AppState, token: &str) -> Result<(), AppError> {
    require_setup_required(state).await?;
    if state.setup_tokens.verify(token) {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

#[utoipa::path(
    get,
    path = "/setup/status",
    responses(
        (status = OK, description = "管理者がまだいない (セットアップが要る)"),
        (status = 404, body = crate::error::ErrorResponse, description = "セットアップ済み・この PC の外からの要求"),
    )
)]
async fn status(_: LocalRequest, State(state): State<AppState>) -> Result<StatusCode, AppError> {
    require_setup_required(&state).await?;
    Ok(StatusCode::OK)
}

#[utoipa::path(
    post,
    path = "/setup/token",
    responses(
        (status = OK, body = String, content_type = "text/plain", description = "発行したトークン"),
        (status = 404, body = crate::error::ErrorResponse, description = "セットアップ済み・この PC の外からの要求"),
    )
)]
async fn issue_token(
    _: LocalRequest,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    require_setup_required(&state).await?;
    let token = state.setup_tokens.issue()?;
    // ADR: 受け取るのはタスクトレイだけなので、JSON を読ませずに済むよう本文をトークンだけにする
    // (トレイは HTTP クライアントの crate を持たない、→ src/tray/http.rs)。
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], token))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct VerifyTokenRequest {
    token: String,
}

#[utoipa::path(
    post,
    path = "/setup/token/verify",
    request_body = VerifyTokenRequest,
    responses(
        (status = 204, description = "使えるトークン"),
        (status = 404, body = crate::error::ErrorResponse, description = "セットアップ済み・トークンが違う・期限切れ・この PC の外からの要求"),
    )
)]
async fn verify_token(
    _: LocalRequest,
    State(state): State<AppState>,
    AppJson(payload): AppJson<VerifyTokenRequest>,
) -> Result<StatusCode, AppError> {
    require_valid_setup_token(&state, &payload.token).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct CreateAdminRequest {
    token: String,
    username: String,
    password: String,
}

#[utoipa::path(
    post,
    path = "/setup/admin",
    request_body = CreateAdminRequest,
    responses(
        (status = 201, body = UserWithRecoveryCodeResponse, description = "作成した管理者 (このセッションでログイン済み) とリカバリコード"),
        (status = 404, body = crate::error::ErrorResponse, description = "セットアップ済み・トークンが違う・期限切れ・この PC の外からの要求"),
        (status = 422, body = crate::error::ErrorResponse, description = "ユーザー名重複・ユーザー名/パスワード未入力"),
    )
)]
async fn create_admin(
    _: LocalRequest,
    session: Session,
    State(state): State<AppState>,
    AppJson(payload): AppJson<CreateAdminRequest>,
) -> Result<(StatusCode, Json<UserWithRecoveryCodeResponse>), AppError> {
    // 「管理者が0人」の確認から作成までを直列化する。確認の後に別の要求が管理者を作ると、
    // この口から2人目を作れてしまう。
    let _guard = state.users_write_lock.lock().await;
    require_valid_setup_token(&state, &payload.token).await?;

    let username = validate::trimmed_non_empty(&payload.username, "username is required")?;
    // 二重入力の一致は画面側の責務 (`POST /admin/users` と同じ)。
    validate::non_empty(&payload.password, "password is required")?;

    // 最初の管理者には、作った直後にリカバリコードを見せる (→ docs/access.md「リカバリコード」)。
    // 作成と同じ INSERT で持たせる。別に書き込むと、その失敗でコードの無い管理者が残る。
    let (recovery_code, hash) = auth::new_recovery_code().await?;
    let created = auth::create_user(
        &state.pool,
        &username,
        &payload.password,
        Role::Admin,
        Some(&hash),
    )
    .await?;
    state.setup_tokens.consume(&payload.token);

    // そのままログインさせる (→ docs/access.md「初回セットアップ」)。
    super::auth::start_session(&session, created.id).await?;

    Ok((
        StatusCode::CREATED,
        Json(UserWithRecoveryCodeResponse {
            user: UserResponse {
                id: created.id,
                username,
                role: Role::Admin,
                has_recovery_code: true,
            },
            recovery_code,
        }),
    ))
}

/// 管理者を作る代わりに、バックアップから戻す (→ docs/access.md「バックアップとリストア」)。まず受け取って目録を返す。
#[utoipa::path(
    post,
    path = "/setup/restore/stage",
    params(("x-setup-token" = String, Header, description = "発行したトークン")),
    request_body(content = StageBackupRequest, content_type = "multipart/form-data"),
    responses(
        (status = OK, body = StagedBackupResponse, description = "受け取ったバックアップの目録"),
        (status = 404, body = crate::error::ErrorResponse, description = "セットアップ済み・トークンが違う・期限切れ・この PC の外からの要求"),
        (status = 422, body = crate::error::ErrorResponse, description = "バックアップではない・新しい版で作られた"),
    )
)]
async fn setup_stage_restore(
    _: LocalRequest,
    State(state): State<AppState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Json<StagedBackupResponse>, AppError> {
    let token = headers
        .get(TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    require_valid_setup_token(&state, token).await?;
    super::backup::stage(&state, multipart).await.map(Json)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct SetupRestoreRequest {
    token: String,
    /// 受け取ったときに返した id。
    id: String,
}

/// 受け取ったバックアップで、今のデータを置き換える。ログインはさせない (戻したデータの管理者でログインし直す)。
#[utoipa::path(
    post,
    path = "/setup/restore",
    request_body = SetupRestoreRequest,
    responses(
        (status = 204, description = "戻した"),
        (status = 404, body = crate::error::ErrorResponse, description = "セットアップ済み・トークンが違う・期限切れ・受け取ったものが無い・この PC の外からの要求"),
        (status = 409, body = crate::error::ErrorResponse, description = "アーカイブの走査中"),
        (status = 422, body = crate::error::ErrorResponse, description = "バックアップが壊れている"),
    )
)]
async fn setup_restore(
    _: LocalRequest,
    session: Session,
    State(state): State<AppState>,
    AppJson(payload): AppJson<SetupRestoreRequest>,
) -> Result<StatusCode, AppError> {
    {
        // 「管理者が0人」の確認から戻し終えるまでを直列化する (`create_admin` と同じ)。
        let _guard = state.users_write_lock.lock().await;
        require_valid_setup_token(&state, &payload.token).await?;
        super::backup::restore_staged(&state, &payload.id).await?;
        state.setup_tokens.consume(&payload.token);
    }
    tracing::info!("restored data from a backup on the setup screen");
    super::backup::end_session(&session).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    // 受け取る大きさに上限を設けない理由は `api::backup::router` と同じ。
    let upload_routes = OpenApiRouter::new()
        .routes(routes!(setup_stage_restore))
        .layer(DefaultBodyLimit::disable());

    OpenApiRouter::new()
        .routes(routes!(status))
        .routes(routes!(issue_token))
        .routes(routes!(verify_token))
        .routes(routes!(create_admin))
        .routes(routes!(setup_restore))
        .merge(upload_routes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issued_token_is_accepted_only_by_its_own_value() {
        let tokens = SetupTokens::default();
        let token = tokens.issue().expect("トークンを発行できなかった");

        assert!(tokens.verify(&token));
        assert!(!tokens.verify(&format!("{token}0")));
        assert!(!tokens.verify(""));
    }

    /// 押し直すたびに新しいトークンを発行するので、前のタブに残った URL は使えなくなる。
    #[test]
    fn issuing_a_token_invalidates_the_previous_one() {
        let tokens = SetupTokens::default();
        let first = tokens.issue().expect("トークンを発行できなかった");
        let second = tokens.issue().expect("トークンを発行できなかった");

        assert_ne!(first, second);
        assert!(!tokens.verify(&first));
        assert!(tokens.verify(&second));
    }

    #[test]
    fn expired_token_is_rejected() {
        let tokens = SetupTokens::default();
        let token = tokens
            .issue_with_ttl(Duration::ZERO)
            .expect("トークンを発行できなかった");

        assert!(!tokens.verify(&token));
    }

    #[test]
    fn consumed_token_cannot_be_used_again() {
        let tokens = SetupTokens::default();
        let token = tokens.issue().expect("トークンを発行できなかった");

        tokens.consume(&token);

        assert!(!tokens.verify(&token));
    }

    /// 違うトークンで使い切りを起こせると、正規の管理者の URL が無効になってしまう。
    #[test]
    fn consume_keeps_the_token_when_the_value_does_not_match() {
        let tokens = SetupTokens::default();
        let token = tokens.issue().expect("トークンを発行できなかった");

        tokens.consume("0000");

        assert!(tokens.verify(&token));
    }
}
