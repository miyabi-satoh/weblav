//! ヘルスチェック・ヘルプ・API 以外のパスの扱い・OpenAPI。

use super::*;

#[sqlx::test]
async fn health_endpoint_returns_ok(pool: SqlitePool) {
    let app = test_app(pool).await;
    let (status, body) = send_anon(app, "GET", "/api/v1/health").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#""status":"ok""#), "{body}");
}

#[sqlx::test]
async fn health_endpoint_returns_503_when_db_unavailable(pool: SqlitePool) {
    let app = test_app(pool.clone()).await;
    pool.close().await;

    let (status, body) = send_anon(app, "GET", "/api/v1/health").await;

    assert_error(
        status,
        &body,
        StatusCode::SERVICE_UNAVAILABLE,
        "database_unavailable",
    );
}

#[sqlx::test]
async fn help_pages_list_is_readable_anonymously(pool: SqlitePool) {
    let app = test_app(pool).await;
    let (status, body) = send_anon(app, "GET", "/api/v1/help/pages").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    // docs/manual/ja/01-intro.md の見出しがタイトルとして拾えていることを確認する。
    assert!(body.contains(r#""slug":"intro""#), "{body}");
    assert!(body.contains(r#""title":"はじめに""#), "{body}");
}

#[sqlx::test]
async fn help_page_is_readable_anonymously(pool: SqlitePool) {
    let app = test_app(pool).await;
    let (status, body) = send_anon(app, "GET", "/api/v1/help/pages/intro").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""slug":"intro""#), "{body}");
    assert!(body.contains("WebLAV"), "{body}");
}

#[sqlx::test]
async fn help_pages_are_returned_in_the_requested_locale(pool: SqlitePool) {
    let app = test_app(pool).await;
    let (status, body) = send_anon(app, "GET", "/api/v1/help/pages?locale=en").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    // docs/manual/en/01-intro.md の見出しが拾えていること。slug は原典と共通。
    assert!(body.contains(r#""slug":"intro""#), "{body}");
    assert!(body.contains(r#""title":"Introduction""#), "{body}");
    // どの言語で返したかを画面が `lang` に使う (→ docs/help.md)。
    assert!(body.contains(r#""locale":"en""#), "{body}");
}

#[sqlx::test]
async fn help_page_falls_back_to_the_source_locale(pool: SqlitePool) {
    let app = test_app(pool).await;
    // 知らないロケールは422にせず原典 (日本語) を返す。
    let (status, body) = send_anon(app, "GET", "/api/v1/help/pages/intro?locale=xx").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""title":"はじめに""#), "{body}");
    // 要求した `xx` ではなく、実際に返した原典が載る。
    assert!(body.contains(r#""locale":"ja""#), "{body}");
}

#[sqlx::test]
async fn help_page_with_unknown_slug_is_not_found(pool: SqlitePool) {
    let app = test_app(pool).await;
    let (status, _) = send_anon(app, "GET", "/api/v1/help/pages/nope").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn help_image_is_readable_anonymously_and_falls_back_to_the_source(pool: SqlitePool) {
    let app = test_app(pool).await;
    // 訳の無い画像は原典 (日本語) のものを返す。
    let response = send_raw(
        app,
        build_request(
            "GET",
            "/api/v1/help/images/intro-overview.webp?locale=xx",
            None,
            None,
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/webp");
}

#[sqlx::test]
async fn help_image_rejects_names_outside_the_images_folder(pool: SqlitePool) {
    let app = test_app(pool).await;
    let response = send_raw(
        app,
        build_request("GET", "/api/v1/help/images/..%2F01-intro.md", None, None),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn unknown_api_path_returns_json_404(pool: SqlitePool) {
    let app = test_app(pool).await;
    let response = send_raw(app, build_request("GET", "/api/v1/nope", None, None)).await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/json")
    );
    let body = body_string(response).await;
    assert!(body.contains(r#""code":"not_found""#), "{body}");
}

#[sqlx::test]
async fn unknown_non_api_path_falls_back_to_spa(pool: SqlitePool) {
    let app = test_app(pool).await;
    let response = send_raw(app, build_request("GET", "/nope", None, None)).await;

    // frontend/build にスタブの index.html しか無い環境でも
    // 404にならず(SPAフォールバックが効いている)、少なくともHTMLとして
    // 返ってくることだけを確認する。
    assert_ne!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/html")
    );
}

/// SPAフォールバック(未登録パス)はセッション層(`route_layer`)の対象外であるべき。
/// ログイン済みCookie付きでフォールバックに当たっても、セッションの
/// SELECT/upsertが起きない(=Set-Cookieで確認できる再保存が起きない)ことを確認する。
#[sqlx::test]
async fn unknown_non_api_path_with_session_cookie_does_not_touch_session(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let response = send_raw(app, build_request("GET", "/nope", Some(&cookie), None)).await;

    assert_ne!(response.status(), StatusCode::NOT_FOUND);
    assert!(
        response.headers().get(header::SET_COOKIE).is_none(),
        "フォールバックにセッション層が掛かっていると、always_saveにより毎回Set-Cookieで再保存される"
    );
}

#[test]
fn openapi_doc_has_prefixed_health_path() {
    let openapi = weblav::api::openapi();
    let json = openapi.to_json().unwrap();
    assert!(json.contains("/api/v1/health"), "{json}");
    assert!(json.contains("/api/v1/auth/login"), "{json}");
    assert!(json.contains("/api/v1/auth/logout"), "{json}");
    assert!(json.contains("/api/v1/auth/me"), "{json}");
    assert!(json.contains("/api/v1/contents"), "{json}");
    assert!(json.contains("/api/v1/admin/contents"), "{json}");
    assert!(json.contains("/api/v1/contents/{id}/browse"), "{json}");
    assert!(json.contains("/api/v1/contents/{id}/download"), "{json}");
    assert!(json.contains("/api/v1/contents/{id}/rescan"), "{json}");
    assert!(json.contains("/api/v1/contents/{id}/axes"), "{json}");
    assert!(
        json.contains("/api/v1/contents/{id}/axes/{axis_id}"),
        "{json}"
    );
    assert!(
        json.contains("/api/v1/contents/{id}/axes/{axis_id}/values"),
        "{json}"
    );
    assert!(json.contains("/api/v1/contents/{id}/archive"), "{json}");
    assert!(json.contains("/api/v1/contents/{id}/items"), "{json}");
    assert!(
        json.contains("/api/v1/contents/{id}/items/{item_id}"),
        "{json}"
    );
    assert!(
        json.contains("/api/v1/contents/{id}/items/{item_id}/download"),
        "{json}"
    );
    assert!(json.contains("/api/v1/admin/users"), "{json}");
    assert!(json.contains("/api/v1/admin/users/{id}/role"), "{json}");
    assert!(json.contains("/api/v1/admin/users/{id}/password"), "{json}");
    assert!(json.contains("/api/v1/site-settings"), "{json}");
    assert!(json.contains("/api/v1/admin/site-settings"), "{json}");
    assert!(json.contains("HealthResponse"), "{json}");
    assert!(json.contains("ErrorResponse"), "{json}");
    assert!(json.contains("LoginRequest"), "{json}");
    assert!(json.contains("UserResponse"), "{json}");
    assert!(json.contains("ContentResponse"), "{json}");
    assert!(json.contains("AdminContentResponse"), "{json}");
    assert!(json.contains("CreateContentRequest"), "{json}");
    assert!(json.contains("UpdateContentRequest"), "{json}");
    assert!(json.contains("AxisResponse"), "{json}");
    assert!(json.contains("AxisValueResponse"), "{json}");
    assert!(json.contains("ArchiveViewResponse"), "{json}");
    assert!(json.contains("AdminArchiveItemResponse"), "{json}");
    assert!(json.contains("UserListItem"), "{json}");
    assert!(!json.contains("utoipa-axum"), "{json}");
    assert!(openapi.info.license.is_none());
}
