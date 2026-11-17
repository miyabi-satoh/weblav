//! 新しいパスワードの下限 (→ docs/access.md「パスワードの下限」)。読むのは誰でも、変えるのは `admin` だけ。

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::error_detail::ValidationDetail;
use super::operations::{Op, OpRecord, changed_fields, record_or_warn};
use crate::auth::ConfirmedAdmin;
use crate::error::{AppError, AppJson};
use crate::state::AppState;

/// 下限の上限。パスワードの長さの上限を設けるサービスでも、少なくとも 64 文字は受けるよう求められる
/// (NIST SP 800-63B-4 3.1.1.2)。これを超える下限は、打ち間違いでだれも新しいパスワードを決められなくなるだけのため。
pub(crate) const MAX_PASSWORD_MIN_LENGTH: u32 = 64;

#[derive(Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PasswordPolicy {
    /// 新しいパスワードの最小の文字数。1〜64。
    #[schema(minimum = 1, maximum = 64)]
    pub password_min_length: u32,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PasswordPolicyResponse {
    /// 新しいパスワードの最小の文字数。
    pub password_min_length: u32,
    /// 既定の最小の文字数。最初のセットアップはこの値で固定する。画面が、セットアップの案内と「既定値に戻す」に使う。
    pub default_password_min_length: u32,
}

// 未ログインでも読める。リカバリコードでの再設定とセットアップの画面が、下限を欄の下に出すため。
#[utoipa::path(
    get,
    path = "/password-policy",
    responses(
        (status = OK, body = PasswordPolicyResponse, description = "新しいパスワードの下限と既定"),
    )
)]
async fn get_password_policy(
    State(state): State<AppState>,
) -> Result<Json<PasswordPolicyResponse>, AppError> {
    Ok(Json(PasswordPolicyResponse {
        password_min_length: min_length(&state.pool).await?,
        default_password_min_length: crate::auth::DEFAULT_PASSWORD_MIN_LENGTH,
    }))
}

#[utoipa::path(
    put,
    path = "/admin/password-policy",
    request_body = PasswordPolicy,
    responses(
        (status = OK, body = PasswordPolicy, description = "更新後の下限"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外・パスワードの確かめ直しが要る"),
        (status = 422, body = crate::error::ErrorResponse, description = "下限が 1〜64 の外"),
    )
)]
async fn update_password_policy(
    admin: ConfirmedAdmin,
    State(state): State<AppState>,
    AppJson(payload): AppJson<PasswordPolicy>,
) -> Result<Json<PasswordPolicy>, AppError> {
    // 0 を許すと空のパスワードが通る。空を断るのは下限によらない決まり。
    if !(1..=MAX_PASSWORD_MIN_LENGTH).contains(&payload.password_min_length) {
        return Err(AppError::Validation(
            "passwordMinLength must be between 1 and 64".to_string(),
        ));
    }
    let before = PasswordPolicy {
        password_min_length: min_length(&state.pool).await?,
    };
    let stored = i64::from(payload.password_min_length);
    sqlx::query!(
        "UPDATE site_settings SET password_min_length = ? WHERE id = 1",
        stored
    )
    .execute(&state.pool)
    .await?;
    let fields = changed_fields(&before, &payload);
    if !fields.is_empty() {
        record_or_warn(
            &state.pool,
            &OpRecord::new(Op::UpdateSiteSettings, &admin).fields(fields),
        )
        .await;
    }
    Ok(Json(payload))
}

/// 今の下限。
pub(crate) async fn min_length(
    executor: impl sqlx::SqliteExecutor<'_>,
) -> Result<u32, sqlx::Error> {
    let stored = sqlx::query_scalar!("SELECT password_min_length FROM site_settings WHERE id = 1")
        .fetch_one(executor)
        .await?;
    // 範囲の中で保存するので外れないが、手で書き換えられた DB でも空のパスワードを通さない。
    Ok(u32::try_from(stored)
        .unwrap_or(MAX_PASSWORD_MIN_LENGTH)
        .clamp(1, MAX_PASSWORD_MIN_LENGTH))
}

/// 新しいパスワードが、今の下限に届いているかを確かめる。
pub(crate) async fn check_new_password(
    pool: &sqlx::SqlitePool,
    password: &str,
) -> Result<(), AppError> {
    check_length(password, min_length(pool).await?)
}

/// 文字で数える (日本語の 8 文字が通る)。前後の空白も、パスワードの一部として数える。
pub(crate) fn meets_min_length(password: &str, min_length: u32) -> bool {
    password.chars().count() >= min_length as usize
}

/// `meets_min_length` を満たさなければ、画面が下限を出せる 422 にする。
pub(crate) fn check_length(password: &str, min_length: u32) -> Result<(), AppError> {
    if !meets_min_length(password, min_length) {
        return Err(AppError::ValidationDetailed {
            message: format!("password must be at least {min_length} characters"),
            detail: ValidationDetail::PasswordTooShort { min_length },
        });
    }
    Ok(())
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_password_policy))
        .routes(routes!(update_password_policy))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_length_accepts_exactly_the_minimum_and_rejects_one_less() {
        assert!(check_length("abcdefgh", 8).is_ok());
        assert!(matches!(
            check_length("abcdefg", 8),
            Err(AppError::ValidationDetailed {
                detail: ValidationDetail::PasswordTooShort { min_length: 8 },
                ..
            })
        ));
    }

    #[test]
    fn check_length_counts_characters_not_bytes() {
        assert!(check_length("あいうえおかきく", 8).is_ok());
        assert!(check_length("あいうえおかき", 8).is_err());
    }

    #[test]
    fn check_length_counts_surrounding_whitespace() {
        assert!(check_length(" abcdef ", 8).is_ok());
    }

    #[test]
    fn check_length_with_the_minimum_of_one_rejects_only_empty() {
        assert!(check_length("a", 1).is_ok());
        assert!(check_length("", 1).is_err());
    }

    #[sqlx::test]
    async fn min_length_defaults_to_the_setup_minimum(pool: sqlx::SqlitePool) {
        assert_eq!(
            min_length(&pool).await.expect("下限を読めなかった"),
            crate::auth::DEFAULT_PASSWORD_MIN_LENGTH
        );
    }
}
