//! ログイン・セッション・レート制限・回復コード。

use super::*;

#[sqlx::test]
async fn login_with_wrong_password_is_invalid_credentials(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let app = test_app(pool).await;

    let (status, body) = send_anon_json(
        app,
        "POST",
        "/api/v1/auth/login",
        r#"{"username":"alice","password":"wrong"}"#,
    )
    .await;

    assert_error(
        status,
        &body,
        StatusCode::UNAUTHORIZED,
        "invalid_credentials",
    );
}

#[sqlx::test]
async fn login_with_malformed_json_returns_envelope(pool: SqlitePool) {
    let app = test_app(pool).await;

    let (status, body) = send_anon_json(app, "POST", "/api/v1/auth/login", "{not json").await;

    assert_error(
        status,
        &body,
        StatusCode::BAD_REQUEST,
        "invalid_request_body",
    );
}

#[sqlx::test]
async fn login_with_missing_field_returns_envelope(pool: SqlitePool) {
    let app = test_app(pool).await;

    let (status, body) =
        send_anon_json(app, "POST", "/api/v1/auth/login", r#"{"username":"alice"}"#).await;

    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
}

#[sqlx::test]
async fn login_with_wrong_content_type_returns_envelope(pool: SqlitePool) {
    let app = test_app(pool).await;

    let response = send_raw(
        app,
        Request::post("/api/v1/auth/login")
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Body::from(r#"{"username":"alice","password":"whatever"}"#))
            .unwrap(),
    )
    .await;

    let status = response.status();
    let body = body_string(response).await;
    assert_error(
        status,
        &body,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "invalid_request_body",
    );
}

#[sqlx::test]
async fn login_with_unknown_user_is_invalid_credentials_not_not_found(pool: SqlitePool) {
    let app = test_app(pool).await;

    let (status, body) = send_anon_json(
        app,
        "POST",
        "/api/v1/auth/login",
        r#"{"username":"nobody","password":"whatever"}"#,
    )
    .await;

    assert_error(
        status,
        &body,
        StatusCode::UNAUTHORIZED,
        "invalid_credentials",
    );
}

/// username は大文字小文字を無視して照合される(`COLLATE NOCASE`)。登録時と異なる
/// 大文字小文字でもログインできることを確認する。
#[sqlx::test]
async fn login_with_different_case_username_succeeds(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let app = test_app(pool).await;

    let (status, _) = send_anon_json(
        app,
        "POST",
        "/api/v1/auth/login",
        r#"{"username":"Alice","password":"correct-password"}"#,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
}

#[sqlx::test]
async fn login_me_logout_round_trip(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let app = test_app(pool).await;

    let login_response = send_raw(
        app.clone(),
        build_request(
            "POST",
            "/api/v1/auth/login",
            None,
            Some(r#"{"username":"alice","password":"correct-password"}"#),
        ),
    )
    .await;
    assert_eq!(login_response.status(), StatusCode::OK);
    let cookie = set_cookie(&login_response);

    let (status, body) = send_empty(app.clone(), "GET", "/api/v1/auth/me", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("alice"), "{body}");
    assert!(body.contains(r#""role":"user""#), "{body}");

    let (status, _) = send_empty(app.clone(), "POST", "/api/v1/auth/logout", &cookie).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = send_empty(app, "GET", "/api/v1/auth/me", &cookie).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn me_returns_admin_role_for_admin_user(pool: SqlitePool) {
    insert_admin(&pool, "admin-alice", "correct-password").await;
    let app = test_app(pool).await;

    let login_response = send_raw(
        app.clone(),
        build_request(
            "POST",
            "/api/v1/auth/login",
            None,
            Some(r#"{"username":"admin-alice","password":"correct-password"}"#),
        ),
    )
    .await;
    assert_eq!(login_response.status(), StatusCode::OK);
    let cookie = set_cookie(&login_response);
    let login_body = body_string(login_response).await;
    assert!(login_body.contains(r#""role":"admin""#), "{login_body}");

    let (status, me_body) = send_empty(app, "GET", "/api/v1/auth/me", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(me_body.contains(r#""role":"admin""#), "{me_body}");
}

/// ログイン後にDBからユーザーが削除された場合、有効なセッションCookieを持っていても
/// `/me` が401を返すことを確認する(`AuthUser` がセッションだけでなくDB上の実在も確認する)。
#[sqlx::test]
async fn me_returns_401_after_user_is_deleted(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let app = test_app(pool.clone()).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    sqlx::query("DELETE FROM users WHERE username = ?")
        .bind("alice")
        .execute(&pool)
        .await
        .expect("failed to delete test user");

    let (status, _) = send_empty(app, "GET", "/api/v1/auth/me", &cookie).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// 同一ユーザー名への連続したログイン失敗が、一定回数を超えると429(Too Many Requests)に
/// なることを確認する。正しいパスワードでの試行も上限超過後は429になる
/// (レートリミットはパスワード検証より前に掛かる)。
#[sqlx::test]
async fn login_is_rate_limited_after_repeated_failures(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let app = test_app(pool).await;

    // レートリミットの上限(5回/分)に達するまでは401。
    for _ in 0..5 {
        let (status, _) = send_anon_json(
            app.clone(),
            "POST",
            "/api/v1/auth/login",
            r#"{"username":"alice","password":"wrong"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    // 上限を超えると、正しいパスワードでも429になる。
    let (status, body) = send_anon_json(
        app,
        "POST",
        "/api/v1/auth/login",
        r#"{"username":"alice","password":"correct-password"}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::TOO_MANY_REQUESTS,
        "too_many_requests",
    );
}

const RECOVERY_CODE_URI: &str = "/api/v1/auth/recovery-code";

/// ログイン中の管理者のリカバリコードを作り、見せる形のコードを返す。
async fn create_recovery_code(app: Router, cookie: &str, password: &str) -> String {
    let (status, body) = send_json(
        app,
        "POST",
        RECOVERY_CODE_URI,
        cookie,
        &format!(r#"{{"currentPassword":"{password}"}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    json_str(&body, "recoveryCode")
}

/// 本人は今のパスワードを入れて自分のユーザー名を変えられ、ログインしたまま新しい名前になる。
#[sqlx::test]
async fn user_changes_their_own_username_with_the_current_password(pool: SqlitePool) {
    insert_user(&pool, "bob", "correct-password").await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "bob", "correct-password").await;

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        "/api/v1/auth/username",
        &cookie,
        r#"{"currentPassword":"wrong","username":"robert"}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNAUTHORIZED,
        "incorrect_current_password",
    );

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        "/api/v1/auth/username",
        &cookie,
        r#"{"currentPassword":"correct-password","username":"robert"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (status, body) = send_empty(app.clone(), "GET", "/api/v1/auth/me", &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""username":"robert""#), "{body}");

    login(app, "robert", "correct-password").await;
}

/// 管理者は今のパスワードを入れてリカバリコードを作れる (→ docs/access.md「リカバリコード」)。
#[sqlx::test]
async fn admin_creates_a_recovery_code_with_the_current_password(pool: SqlitePool) {
    insert_admin(&pool, "admin", "correct-password").await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "admin", "correct-password").await;

    let (_, body) = send_empty(app.clone(), "GET", "/api/v1/auth/me", &cookie).await;
    assert!(body.contains(r#""hasRecoveryCode":false"#), "{body}");

    let (status, body) = send_json(
        app.clone(),
        "POST",
        RECOVERY_CODE_URI,
        &cookie,
        r#"{"currentPassword":"wrong"}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNAUTHORIZED,
        "incorrect_current_password",
    );

    let code = create_recovery_code(app.clone(), &cookie, "correct-password").await;
    // 5文字ずつ `-` で区切った 20 文字。
    assert_eq!(code.len(), 23, "{code}");
    assert_eq!(code.split('-').count(), 4, "{code}");

    let (_, body) = send_empty(app, "GET", "/api/v1/auth/me", &cookie).await;
    assert!(body.contains(r#""hasRecoveryCode":true"#), "{body}");
}

/// 編集者も自分のコードを作り、それでパスワードを再設定できる。
#[sqlx::test]
async fn regular_user_resets_the_password_with_their_own_recovery_code(pool: SqlitePool) {
    insert_user(&pool, "alice", "old-password").await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "old-password").await;
    let code = create_recovery_code(app.clone(), &cookie, "old-password").await;

    let response = recover(app.clone(), "alice", &code, "new-password").await;
    assert_eq!(response.status(), StatusCode::OK);
    login(app, "alice", "new-password").await;
}

/// コードで再設定すると、新しいパスワードでログインした状態になり、コードは新しいものに替わる。
#[sqlx::test]
async fn recovery_code_resets_the_password_and_is_replaced(pool: SqlitePool) {
    insert_admin(&pool, "admin", "old-password").await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "admin", "old-password").await;
    let code = create_recovery_code(app.clone(), &cookie, "old-password").await;

    let response = recover(app.clone(), "admin", &code, "new-password").await;
    assert_eq!(response.status(), StatusCode::OK);
    let recovered_cookie = set_cookie(&response);
    let body = body_string(response).await;
    let new_code = json_str(&body, "recoveryCode");
    assert_ne!(new_code, code);

    let (status, body) = send_empty(app.clone(), "GET", "/api/v1/auth/me", &recovered_cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#""username":"admin""#), "{body}");
    login(app.clone(), "admin", "new-password").await;

    // 使ったコードは使えない。新しいコードは使える。
    let response = recover(app.clone(), "admin", &code, "another").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = recover(app, "admin", &new_code, "another").await;
    assert_eq!(response.status(), StatusCode::OK);
}

/// ユーザーがいない・コードが無い・違う、を区別せず同じ応答にする。
#[sqlx::test]
async fn recovery_failures_are_indistinguishable(pool: SqlitePool) {
    insert_admin(&pool, "admin", "password").await;
    insert_admin(&pool, "no-code", "password").await;
    let app = test_app(pool.clone()).await;
    let cookie = login(app.clone(), "admin", "password").await;
    let code = create_recovery_code(app.clone(), &cookie, "password").await;

    let wrong = "00000-00000-00000-00000";
    for (username, code) in [
        ("admin", wrong),
        ("admin", "not-a-code"),
        ("nobody", code.as_str()),
        ("no-code", code.as_str()),
    ] {
        let response = recover(app.clone(), username, code, "new-password").await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{username} {code}"
        );
        let body = body_string(response).await;
        assert!(body.contains(r#""code":"invalid_recovery_code""#), "{body}");
    }
    login(app, "admin", "password").await;
}

/// ロールを変えてもコードは消えず、編集者に下げた後もそのコードで再設定できる。
#[sqlx::test]
async fn changing_the_role_keeps_the_recovery_code(pool: SqlitePool) {
    insert_admin(&pool, "admin", "password").await;
    insert_admin(&pool, "bob", "password").await;
    let app = test_app_pro(pool.clone()).await;
    let admin_cookie = login(app.clone(), "admin", "password").await;
    let bob_cookie = login(app.clone(), "bob", "password").await;
    let code = create_recovery_code(app.clone(), &bob_cookie, "password").await;
    let bob_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'bob'")
        .fetch_one(&pool)
        .await
        .expect("bob should exist");

    let uri = format!("/api/v1/admin/users/{bob_id}/role");
    let (status, _) = send_json(
        app.clone(),
        "PUT",
        &uri,
        &admin_cookie,
        r#"{"role":"user"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let response = recover(app, "bob", &code, "new-password").await;
    assert_eq!(response.status(), StatusCode::OK);
}

/// 管理者がパスワードを再設定すると、本人のコードは使えなくなる。
#[sqlx::test]
async fn resetting_the_password_by_an_admin_discards_the_recovery_code(pool: SqlitePool) {
    insert_admin(&pool, "admin", "password").await;
    insert_user(&pool, "alice", "password").await;
    let app = test_app(pool.clone()).await;
    let admin_cookie = login(app.clone(), "admin", "password").await;
    let alice_cookie = login(app.clone(), "alice", "password").await;
    let code = create_recovery_code(app.clone(), &alice_cookie, "password").await;
    let alice_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'alice'")
        .fetch_one(&pool)
        .await
        .expect("alice should exist");

    let (status, _) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/admin/users/{alice_id}/password"),
        &admin_cookie,
        r#"{"password":"reset-password"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let response = recover(app, "alice", &code, "new-password").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// 再設定の試行はログインと別の枠で絞る。こちらを使い切っても、ログインはできる。
#[sqlx::test]
async fn recovery_is_rate_limited_separately_from_login(pool: SqlitePool) {
    insert_admin(&pool, "admin", "password").await;
    let app = test_app(pool).await;

    for _ in 0..5 {
        let response = recover(app.clone(), "admin", "00000-00000-00000-00000", "x").await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let response = recover(app.clone(), "admin", "00000-00000-00000-00000", "x").await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

    login(app, "admin", "password").await;
}
