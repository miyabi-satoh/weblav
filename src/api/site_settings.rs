//! サイト全体の設定 API (→ docs/access.md「ロールと操作」・docs/architecture.md「スキーマ」)。読むのは誰でも、変えるのは `AdminUser` だけ。

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AdminUser;
use crate::error::{AppError, AppJson};
use crate::state::AppState;

/// 1項目の上限 (文字数)。ホームの見出しが数行に収まる長さ。
const MAX_CHARS: usize = 100;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SiteSettingsResponse {
    /// ホームの見出しの上に小さく出す名前。未設定は空文字列。
    pub site_name: String,
    /// ホームの一覧の上に大きく出す見出し。未設定は空文字列。
    pub home_heading: String,
}

// 未ログインでも読める。ホームは匿名でも開けるため (→ docs/access.md「匿名閲覧の受け口」)。
#[utoipa::path(
    get,
    path = "/site-settings",
    responses(
        (status = OK, body = SiteSettingsResponse, description = "サイト全体の設定"),
    )
)]
async fn get_site_settings(
    State(state): State<AppState>,
) -> Result<Json<SiteSettingsResponse>, AppError> {
    Ok(Json(load(&state.pool).await?))
}

/// 今のサイトの設定。Pro の窓口に出す名前 (→ `api::pro`) にも使う。
pub(crate) async fn load(pool: &sqlx::SqlitePool) -> Result<SiteSettingsResponse, sqlx::Error> {
    sqlx::query_as!(
        SiteSettingsResponse,
        r#"SELECT site_name as "site_name!", home_heading as "home_heading!"
           FROM site_settings WHERE id = 1"#
    )
    .fetch_one(pool)
    .await
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateSiteSettingsRequest {
    site_name: String,
    home_heading: String,
}

#[utoipa::path(
    put,
    path = "/admin/site-settings",
    request_body = UpdateSiteSettingsRequest,
    responses(
        (status = OK, body = SiteSettingsResponse, description = "更新後の設定"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 422, body = crate::error::ErrorResponse, description = "上限の文字数を超えている"),
    )
)]
async fn update_site_settings(
    _admin: AdminUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<UpdateSiteSettingsRequest>,
) -> Result<Json<SiteSettingsResponse>, AppError> {
    // 空白だけの入力は未設定として扱う。画面に空の行を出さないため。
    let site_name = within_limit(payload.site_name.trim(), "siteName is too long")?;
    let home_heading = within_limit(payload.home_heading.trim(), "homeHeading is too long")?;

    let settings = sqlx::query_as!(
        SiteSettingsResponse,
        r#"UPDATE site_settings SET site_name = ?, home_heading = ? WHERE id = 1
           RETURNING site_name as "site_name!", home_heading as "home_heading!""#,
        site_name,
        home_heading,
    )
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(settings))
}

fn within_limit<'a>(value: &'a str, message: &str) -> Result<&'a str, AppError> {
    if value.chars().count() > MAX_CHARS {
        return Err(AppError::Validation(message.to_string()));
    }
    Ok(value)
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_site_settings))
        .routes(routes!(update_site_settings))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// バイト数でなく文字で数える (日本語の 100 文字が通る)。
    #[test]
    fn within_limit_counts_characters_not_bytes() {
        let at_limit = "あ".repeat(MAX_CHARS);
        assert_eq!(
            within_limit(&at_limit, "too long").expect("通るはず"),
            at_limit
        );
        assert!(matches!(
            within_limit(&"あ".repeat(MAX_CHARS + 1), "too long"),
            Err(AppError::Validation(_))
        ));
        assert!(within_limit("", "too long").is_ok());
    }
}
