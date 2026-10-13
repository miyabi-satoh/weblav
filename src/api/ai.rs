//! AI の機能の設定の口: PC の登録・誰が使えるか・使った量 (→ docs/ai.md)。
//! 窓口とのやり取りと登録の状態は `crate::ai`。

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::ai::{self, Account};
use crate::auth::{AdminUser, AuthUser};
use crate::error::{AppError, run_blocking};
use crate::state::AppState;

/// PC の登録の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum AiLinkState {
    /// 登録していない。
    Unlinked,
    /// 登録の途中。WebLAV のアカウントの画面で認めるのを待っている。
    Pending,
    /// 登録済み。
    Linked,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AiLinkResponse {
    state: AiLinkState,
    /// 登録の途中のとき、管理者が開いて登録を認める画面の URL。
    link_url: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AiStatusResponse {
    state: AiLinkState,
    /// 登録の途中のとき、管理者が開いて登録を認める画面の URL。
    link_url: Option<String>,
    /// 登録済みのとき、登録先のアカウントのメールアドレス。アカウントにつながらなければ無い。
    email: Option<String>,
    /// 登録済みのとき、残りの割合 (0〜100)。アカウントにつながらなければ無い。
    remaining_percent: Option<f64>,
    /// 登録済みだが、アカウントにつながらず残りを確かめられなかった。
    account_unreachable: bool,
    /// 残りを足す・登録した PC を見る、アカウントのページの URL。
    account_url: String,
}

/// 今の登録の状態と、登録の途中なら認める画面の URL。
async fn link_response(state: &AppState) -> Result<AiLinkResponse, AppError> {
    Ok(match state.ai.account() {
        None => AiLinkResponse {
            state: AiLinkState::Unlinked,
            link_url: None,
        },
        Some(Account::Pending { code_verifier, .. }) => {
            let site_name = super::site_settings::load(&state.pool).await?.site_name;
            // 窓口のアカウントの画面で、どの PC かを見分けるための名前。読めなければ空にする (登録はできる)。
            let pc_name = crate::mdns::os_host_label().unwrap_or_default();
            AiLinkResponse {
                state: AiLinkState::Pending,
                link_url: Some(ai::link_url(
                    state.ai.server(),
                    &code_verifier,
                    &site_name,
                    &pc_name,
                )),
            }
        }
        Some(Account::Linked { .. }) => AiLinkResponse {
            state: AiLinkState::Linked,
            link_url: None,
        },
    })
}

#[utoipa::path(
    get,
    path = "/admin/ai",
    responses(
        (status = OK, body = AiStatusResponse, description = "PC の登録の状態と、登録済みなら残り"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
    )
)]
async fn get_status(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<AiStatusResponse>, AppError> {
    let balance = if state.ai.is_linked() {
        let ai = Arc::clone(&state.ai);
        Some(run_blocking(move || ai.balance()).await?)
    } else {
        None
    };
    let (email, remaining_percent, account_unreachable) = match balance {
        Some(Ok(balance)) => (balance.email, Some(balance.remaining_percent), false),
        // 窓口の側で登録を外されていた。手元のトークンは消えているので、下で未登録として返る。
        Some(Err(AppError::AiNotLinked)) | None => (None, None, false),
        Some(Err(err)) => {
            tracing::warn!(%err, "failed to get the balance from the account server");
            (None, None, true)
        }
    };
    let link = link_response(&state).await?;
    Ok(Json(AiStatusResponse {
        state: link.state,
        link_url: link.link_url,
        email,
        remaining_percent,
        account_unreachable,
        account_url: ai::account_url(state.ai.server()),
    }))
}

/// PC の登録を始める。途中のものがあれば、同じ URL を返す。
#[utoipa::path(
    post,
    path = "/admin/ai/link",
    responses(
        (status = OK, body = AiLinkResponse, description = "登録の途中になった。認める画面の URL を返す"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 409, body = crate::error::ErrorResponse, description = "登録済み"),
    )
)]
async fn begin_link(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<AiLinkResponse>, AppError> {
    let ai = Arc::clone(&state.ai);
    if run_blocking(move || ai.begin_link()).await??.is_none() {
        return Err(AppError::Conflict(
            "this PC is already linked to a WebLAV account".to_string(),
        ));
    }
    Ok(Json(link_response(&state).await?))
}

/// 登録の途中なら、認めたかをアカウントに問い合わせる。画面が数秒おきに呼ぶ。
#[utoipa::path(
    post,
    path = "/admin/ai/link/poll",
    responses(
        (status = OK, body = AiLinkResponse, description = "問い合わせた後の状態。認めていれば `linked`、まだなら `pending`"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 429, body = crate::error::ErrorResponse, description = "問い合わせすぎ"),
        (status = 502, body = crate::error::ErrorResponse, description = "アカウントにつながらない"),
    )
)]
async fn poll_link(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<AiLinkResponse>, AppError> {
    let ai = Arc::clone(&state.ai);
    run_blocking(move || ai.poll_link()).await??;
    Ok(Json(link_response(&state).await?))
}

#[utoipa::path(
    delete,
    path = "/admin/ai/link",
    responses(
        (status = 204, description = "登録の途中を取り消した (途中でなければ何もしない)"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
    )
)]
async fn cancel_link(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    let ai = Arc::clone(&state.ai);
    run_blocking(move || ai.cancel_link()).await??;
    Ok(StatusCode::NO_CONTENT)
}

/// PC の登録を解除する。アカウントにつながらなくても、この PC の側は未登録に戻す。
#[utoipa::path(
    delete,
    path = "/admin/ai",
    responses(
        (status = 204, description = "登録を解除した (登録済みでなければ何もしない)"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
    )
)]
async fn unlink(_admin: AdminUser, State(state): State<AppState>) -> Result<StatusCode, AppError> {
    let ai = Arc::clone(&state.ai);
    run_blocking(move || ai.unlink()).await??;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AiUsageItem {
    /// 使ったユーザー。消したユーザーの分は無い。
    user_id: Option<i64>,
    /// 今のユーザー名。消したユーザーの分は、使った時点の名前。
    username: String,
    /// 使った回数。
    count: i64,
    /// 使った量の合計。1回の購入で付く量を 1,000,000 とした整数。
    used: i64,
    /// 最後に使った日時 (RFC 3339)。
    last_used_at: String,
}

/// 誰がどれだけ使ったか。量の多い順。
#[utoipa::path(
    get,
    path = "/admin/ai/usage",
    responses(
        (status = OK, body = Vec<AiUsageItem>, description = "ユーザーごとの回数・量の合計・最後に使った日時"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
    )
)]
async fn list_usage(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<AiUsageItem>>, AppError> {
    // 消したユーザーの分 (user_id が NULL) は、使った時点の名前ごとにまとめる。
    let rows = sqlx::query_as!(
        AiUsageItem,
        r#"SELECT ai_usage.user_id as "user_id: i64",
                  COALESCE(users.username, ai_usage.username) as "username!: String",
                  COUNT(*) as "count!: i64",
                  SUM(ai_usage.used) as "used!: i64",
                  MAX(ai_usage.created_at) as "last_used_at!: String"
           FROM ai_usage LEFT JOIN users ON users.id = ai_usage.user_id
           GROUP BY ai_usage.user_id,
                    CASE WHEN ai_usage.user_id IS NULL THEN ai_usage.username END
           ORDER BY 4 DESC, 2 COLLATE NOCASE"#
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AiAvailabilityResponse {
    /// この PC が登録済みで、ログインしているユーザーが AI を使えるか。
    available: bool,
}

/// AI を使うボタンを出すかを、画面が決めるための口。アカウントには問い合わせない (残りが無いことは、使ったときに分かる)。
#[utoipa::path(
    get,
    path = "/ai/availability",
    responses(
        (status = OK, body = AiAvailabilityResponse, description = "AI を使えるか"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
    )
)]
async fn get_availability(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<AiAvailabilityResponse>, AppError> {
    let available = state.ai.is_linked() && ai::is_allowed(&state.pool, &user).await?;
    Ok(Json(AiAvailabilityResponse { available }))
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_status, unlink))
        .routes(routes!(begin_link, cancel_link))
        .routes(routes!(poll_link))
        .routes(routes!(list_usage))
        .routes(routes!(get_availability))
}
