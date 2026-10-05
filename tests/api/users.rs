//! ユーザーの管理。

use super::*;

#[sqlx::test]
async fn users_list_includes_role_and_omits_password_hash(pool: SqlitePool) {
    insert_user(&pool, "bob", "correct-password").await;
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_empty(app, "GET", "/api/v1/admin/users", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    let users = json.as_array().expect("配列でない");
    assert_eq!(users.len(), 2, "{json}");
    let bob = users
        .iter()
        .find(|u| u["username"] == "bob")
        .expect("bobが見つからない");
    assert_eq!(bob["role"], "user");
    assert!(bob.get("passwordHash").is_none(), "{json}");
    assert!(bob["createdAt"].is_string(), "{json}");
}

#[sqlx::test]
async fn users_create_by_admin_succeeds_and_new_user_can_login(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/admin/users",
        &cookie,
        r#"{"username":"newbie","password":"new-password","role":"user"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["username"], "newbie");
    assert_eq!(json["role"], "user");

    login(app, "newbie", "new-password").await;
}

#[sqlx::test]
async fn users_create_with_duplicate_username_is_validation_error(pool: SqlitePool) {
    insert_admin(&pool, "admin-alice", "correct-password").await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "admin-alice", "correct-password").await;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/admin/users",
        &cookie,
        r#"{"username":"admin-alice","password":"new-password","role":"user"}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
}

#[sqlx::test]
async fn users_create_with_empty_username_is_validation_error(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/admin/users",
        &cookie,
        r#"{"username":"","password":"new-password","role":"user"}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
}

#[sqlx::test]
async fn users_create_trims_surrounding_whitespace_from_username(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/admin/users",
        &cookie,
        r#"{"username":"  newbie  ","password":"new-password","role":"user"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["username"], "newbie");

    login(app, "newbie", "new-password").await;
}

#[sqlx::test]
async fn users_create_with_empty_password_is_validation_error(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/admin/users",
        &cookie,
        r#"{"username":"newbie","password":"","role":"user"}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
}

#[sqlx::test]
async fn users_role_update_promotes_and_demotes(pool: SqlitePool) {
    insert_admin(&pool, "admin-alice", "correct-password").await;
    insert_admin(&pool, "admin-carol", "correct-password").await;
    insert_user(&pool, "bob", "correct-password").await;
    let bob_db_id = db_user_id(&pool, "bob").await;
    insert_link_by(&pool, "ボブのメモ", "private", Some(bob_db_id), None).await;
    let app = test_app_pro(pool).await;
    let cookie = login(app.clone(), "admin-alice", "correct-password").await;
    let bob_id = user_id(app.clone(), &cookie, "bob").await;

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/admin/users/{bob_id}/role"),
        &cookie,
        r#"{"role":"admin"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["role"], "admin");
    // 権限変更の応答も一覧と同じ形で、本人のみの件数を含む (→ docs/access.md「ユーザーの削除と作成者」)。
    assert_eq!(json["privateContentCount"], 1, "{json}");

    // adminが複数(alice/carol/bob)いる状態での降格は許可される。
    let (status, body) = send_json(
        app,
        "PUT",
        &format!("/api/v1/admin/users/{bob_id}/role"),
        &cookie,
        r#"{"role":"user"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["role"], "user");
}

#[sqlx::test]
async fn users_role_update_of_sole_admin_is_rejected(pool: SqlitePool) {
    insert_admin(&pool, "admin-alice", "correct-password").await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "admin-alice", "correct-password").await;
    let alice_id = user_id(app.clone(), &cookie, "admin-alice").await;

    let (status, body) = send_json(
        app,
        "PUT",
        &format!("/api/v1/admin/users/{alice_id}/role"),
        &cookie,
        r#"{"role":"user"}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
}

#[sqlx::test]
async fn users_delete_of_sole_admin_is_rejected(pool: SqlitePool) {
    insert_admin(&pool, "admin-alice", "correct-password").await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "admin-alice", "correct-password").await;
    let alice_id = user_id(app.clone(), &cookie, "admin-alice").await;

    let (status, _) = send_empty(
        app,
        "DELETE",
        &format!("/api/v1/admin/users/{alice_id}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

/// TOCTOU回帰テスト: adminが2人だけの状態で、片方を降格するリクエストともう片方を
/// 削除するリクエストを`tokio::join!`で同時に実行する。`AppState::users_write_lock`が
/// 無いと、両リクエストがそれぞれ「admin数=2」を見た直後に両方コミットされ、
/// adminが0人になってしまう(update_role/delete_userのreject_if_last_admin参照)。
/// `users_write_lock`でカウント〜更新が直列化されている限り、どちらか一方だけが
/// 成功し、もう一方は最後のadmin保護で422になるはず。
///
/// 注意: `concurrent_delete_and_upload_of_same_blob_does_not_orphan_row`と同じく、
/// `tokio::join!`は必ずしも危険な順序を再現しないが、回帰時にはいずれかの実行順序で
/// 偶発的に失敗するようになるため安全網として有用。
#[sqlx::test]
async fn concurrent_demote_and_delete_of_last_two_admins_keeps_one_admin(pool: SqlitePool) {
    insert_admin(&pool, "admin-a", "correct-password").await;
    insert_admin(&pool, "admin-b", "correct-password").await;
    let db = pool.clone();
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "admin-a", "correct-password").await;
    let admin_a_id = user_id(app.clone(), &cookie, "admin-a").await;
    let admin_b_id = user_id(app.clone(), &cookie, "admin-b").await;

    let demote_fut = {
        let app = app.clone();
        let cookie = cookie.clone();
        async move {
            send_json(
                app,
                "PUT",
                &format!("/api/v1/admin/users/{admin_a_id}/role"),
                &cookie,
                r#"{"role":"user"}"#,
            )
            .await
        }
    };
    let delete_fut = {
        let app = app.clone();
        let cookie = cookie.clone();
        async move {
            send_empty(
                app,
                "DELETE",
                &format!("/api/v1/admin/users/{admin_b_id}"),
                &cookie,
            )
            .await
        }
    };

    let ((demote_status, demote_body), (delete_status, delete_body)) =
        tokio::join!(demote_fut, delete_fut);

    let successes = [demote_status, delete_status]
        .iter()
        .filter(|s| s.is_success())
        .count();
    assert_eq!(
        successes, 1,
        "demote={demote_status} {demote_body}, delete={delete_status} {delete_body}"
    );

    // どちらのユーザーのセッションで確認すればよいか(成功した方は権限が変わり得る)が
    // 定まらないため、最終状態はDBを直接見る。
    let admin_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'admin'")
        .fetch_one(&db)
        .await
        .expect("admin数を取得できなかった");
    assert_eq!(admin_count, 1, "最後の1人のadminは残るはず");
}

#[sqlx::test]
async fn users_delete_removes_a_non_admin_user(pool: SqlitePool) {
    insert_user(&pool, "bob", "correct-password").await;
    let (app, cookie) = admin_app(&pool).await;
    let bob_id = user_id(app.clone(), &cookie, "bob").await;

    let (status, _) = send_empty(
        app.clone(),
        "DELETE",
        &format!("/api/v1/admin/users/{bob_id}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, body) = send_empty(app, "GET", "/api/v1/admin/users", &cookie).await;
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(
        json.as_array()
            .unwrap()
            .iter()
            .all(|u| u["username"] != "bob"),
        "{json}"
    );
}

#[sqlx::test]
async fn users_reset_password_succeeds_and_old_password_stops_working(pool: SqlitePool) {
    insert_user(&pool, "bob", "old-password").await;
    let (app, cookie) = admin_app(&pool).await;
    let bob_id = user_id(app.clone(), &cookie, "bob").await;

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/admin/users/{bob_id}/password"),
        &cookie,
        r#"{"password":"new-password"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    login(app.clone(), "bob", "new-password").await;

    let (status, _) = send_anon_json(
        app,
        "POST",
        "/api/v1/auth/login",
        r#"{"username":"bob","password":"old-password"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn users_reset_password_with_empty_password_is_validation_error(pool: SqlitePool) {
    insert_user(&pool, "bob", "old-password").await;
    let (app, cookie) = admin_app(&pool).await;
    let bob_id = user_id(app.clone(), &cookie, "bob").await;

    let (status, body) = send_json(
        app,
        "PUT",
        &format!("/api/v1/admin/users/{bob_id}/password"),
        &cookie,
        r#"{"password":""}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
}

#[sqlx::test]
async fn users_rename_by_admin_returns_the_updated_row(pool: SqlitePool) {
    insert_user(&pool, "bob", "correct-password").await;
    let (app, cookie) = admin_app(&pool).await;
    let bob_id = user_id(app.clone(), &cookie, "bob").await;

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/admin/users/{bob_id}/username"),
        &cookie,
        r#"{"username":"  robert  "}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let json: serde_json::Value = serde_json::from_str(&body).expect("JSON のはず");
    assert_eq!(json["id"], bob_id);
    assert_eq!(json["username"], "robert");

    login(app, "robert", "correct-password").await;
}

#[sqlx::test]
async fn users_rename_to_a_taken_username_reports_the_name(pool: SqlitePool) {
    insert_user(&pool, "bob", "correct-password").await;
    let (app, cookie) = admin_app(&pool).await;
    let bob_id = user_id(app.clone(), &cookie, "bob").await;

    let (status, body) = send_json(
        app,
        "PUT",
        &format!("/api/v1/admin/users/{bob_id}/username"),
        &cookie,
        r#"{"username":"ADMIN-ALICE"}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
    let json: serde_json::Value = serde_json::from_str(&body).expect("JSON のはず");
    assert_eq!(json["error"]["detail"]["kind"], "usernameTaken", "{json}");
    assert_eq!(json["error"]["detail"]["name"], "ADMIN-ALICE", "{json}");
}
