use axum::Json;
use axum::extract::State;
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    /// ビルド番号。画面は版に添えて出す (→ docs/distribution.md「版の番号」)。
    build: Option<&'static str>,
}

// `path` はこのルーター自身から見た相対パス。`/api/v1` プレフィックスは
// `api::routed()` 側で `OpenApiRouter::nest` するときに(axumのルーティングと
// OpenAPIドキュメントの両方に)自動的に付与される。
#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = OK, body = HealthResponse),
        (status = 503, body = crate::error::ErrorResponse),
    )
)]
async fn health(State(state): State<AppState>) -> Result<Json<HealthResponse>, AppError> {
    // DBに実際に到達できるかまで確認する(単に起動しているだけでなく)。
    sqlx::query_scalar!("SELECT 1")
        .fetch_one(&state.pool)
        .await?;

    Ok(Json(HealthResponse {
        status: "ok",
        version: crate::APP_VERSION,
        build: crate::APP_BUILD,
    }))
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(health))
}
