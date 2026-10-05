//! 初回セットアップ (→ docs/access.md「初回セットアップ」)

use super::*;

const SETUP_VERIFY_URI: &str = "/api/v1/setup/token/verify";
const SETUP_ADMIN_URI: &str = "/api/v1/setup/admin";

fn setup_admin_request(token: &str, username: &str, password: &str) -> Request<Body> {
    from_loopback(build_request(
        "POST",
        SETUP_ADMIN_URI,
        None,
        Some(&format!(
            r#"{{"token":"{token}","username":"{username}","password":"{password}"}}"#
        )),
    ))
}

#[sqlx::test]
async fn setup_status_is_ok_while_there_is_no_admin(pool: SqlitePool) {
    // admin ではない利用者がいても、セットアップは要る。
    insert_user(&pool, "taro", "password").await;
    let app = test_app(pool).await;

    let (status, _) = send(
        app,
        from_loopback(build_request("GET", SETUP_STATUS_URI, None, None)),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
}

#[sqlx::test]
async fn setup_status_is_not_found_once_an_admin_exists(pool: SqlitePool) {
    insert_admin(&pool, "admin", "correct-password").await;
    let app = test_app(pool).await;

    let (status, _) = send(
        app,
        from_loopback(build_request("GET", SETUP_STATUS_URI, None, None)),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn setup_creates_the_first_admin_and_logs_in(pool: SqlitePool) {
    let app = test_app(pool.clone()).await;
    let token = issue_setup_token(app.clone()).await;

    let (status, _) = send(
        app.clone(),
        from_loopback(build_request(
            "POST",
            SETUP_VERIFY_URI,
            None,
            Some(&format!(r#"{{"token":"{token}"}}"#)),
        )),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let response = send_raw(
        app.clone(),
        setup_admin_request(&token, "satoh", "correct-password"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    // 作成と同時にログイン済みになる。
    let cookie = set_cookie(&response);
    let body = body_string(response).await;
    assert!(body.contains(r#""role":"admin""#), "{body}");
    // 最初の管理者には、作った直後にリカバリコードを返す (→ docs/access.md「リカバリコード」)。
    let code = json_str(&body, "recoveryCode");

    let (status, body) = send_empty(app.clone(), "GET", "/api/v1/auth/me", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#""username":"satoh""#), "{body}");
    assert!(body.contains(r#""hasRecoveryCode":true"#), "{body}");
    let response = recover(app.clone(), "satoh", &code, "new-password").await;
    assert_eq!(response.status(), StatusCode::OK);

    // 管理者ができたので、セットアップの口は閉じる。
    let (status, _) = send(
        app,
        from_loopback(build_request("GET", SETUP_STATUS_URI, None, None)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// 管理者ができたあとは、同じトークンでも2人目を作れない。
/// (トークンを使い切ること自体は `api::setup` の単体テストで確かめている。)
#[sqlx::test]
async fn setup_cannot_create_a_second_admin(pool: SqlitePool) {
    let app = test_app(pool).await;
    let token = issue_setup_token(app.clone()).await;

    let (status, _) = send(
        app.clone(),
        setup_admin_request(&token, "satoh", "correct-password"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = send(app, setup_admin_request(&token, "another", "password")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn setup_rejects_a_token_that_was_not_issued(pool: SqlitePool) {
    let app = test_app(pool.clone()).await;
    issue_setup_token(app.clone()).await;

    let (status, _) = send(
        app,
        setup_admin_request(&"0".repeat(64), "satoh", "correct-password"),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    let admins: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .expect("ユーザー数を数えられなかった");
    assert_eq!(admins, 0);
}

/// 押し直すと新しいトークンになり、前のタブに残った URL は使えなくなる。
#[sqlx::test]
async fn issuing_a_setup_token_invalidates_the_previous_one(pool: SqlitePool) {
    let app = test_app(pool).await;
    let first = issue_setup_token(app.clone()).await;
    let second = issue_setup_token(app.clone()).await;
    assert_ne!(first, second);

    let (status, _) = send(
        app.clone(),
        setup_admin_request(&first, "satoh", "correct-password"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(
        app,
        setup_admin_request(&second, "satoh", "correct-password"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
}

#[sqlx::test]
async fn setup_verify_rejects_a_token_that_was_not_issued(pool: SqlitePool) {
    let app = test_app(pool).await;
    issue_setup_token(app.clone()).await;

    let (status, _) = send(
        app,
        from_loopback(build_request(
            "POST",
            SETUP_VERIFY_URI,
            None,
            Some(&format!(r#"{{"token":"{}"}}"#, "0".repeat(64))),
        )),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn setup_rejects_an_empty_username_or_password(pool: SqlitePool) {
    let app = test_app(pool).await;
    let token = issue_setup_token(app.clone()).await;

    let (status, body) = send(app.clone(), setup_admin_request(&token, " ", "password")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (status, body) = send(app, setup_admin_request(&token, "satoh", "")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

#[sqlx::test]
async fn setup_rejects_a_username_that_is_already_taken(pool: SqlitePool) {
    insert_user(&pool, "taro", "password").await;
    let app = test_app(pool).await;
    let token = issue_setup_token(app.clone()).await;

    let (status, body) = send(app, setup_admin_request(&token, "taro", "correct-password")).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}
