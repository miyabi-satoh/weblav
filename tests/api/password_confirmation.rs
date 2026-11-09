//! 操作の前のパスワードの確かめ直し (→ docs/access.md「操作の前のパスワードの確認」)。
//! 確かめた内かどうかの判定は `src/auth.rs` の単体テストで場合ごとに確かめ、ここでは入口ごとに
//! つながっていることを確かめる。確かめた内でない状態は、確かめた後にパスワードを変えて作る
//! (時刻を巻き戻す口は持たない)。

use super::*;

/// 確かめた後にパスワードが変わった状態を作る (ほかの管理者の再設定と同じ書き換え)。
async fn replace_password_hash(pool: &SqlitePool, username: &str, password: &str) {
    let hash = auth::hash_password(password).expect("ハッシュを作れるはず");
    sqlx::query("UPDATE users SET password_hash = ? WHERE username = ?")
        .bind(hash)
        .bind(username)
        .execute(pool)
        .await
        .expect("パスワードを書き換えられるはず");
}

/// 管理者に再設定されたら、古いパスワードで確かめたセッションは、自分のアカウントの操作を通せない。
/// 再設定でリカバリコードを消しても、古いパスワードを知った人に作り直されないため。
#[sqlx::test]
async fn admin_reset_stops_the_old_session_from_changing_the_account(pool: SqlitePool) {
    insert_admin(&pool, "admin", "admin-password").await;
    insert_user(&pool, "alice", "old-password").await;
    let app = test_app(pool.clone()).await;
    let admin = login(app.clone(), "admin", "admin-password").await;
    let alice = login(app.clone(), "alice", "old-password").await;
    let alice_id = db_user_id(&pool, "alice").await;

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/admin/users/{alice_id}/password"),
        &admin,
        r#"{"password":"new-password"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (status, body) =
        send_empty(app.clone(), "POST", "/api/v1/auth/recovery-code", &alice).await;
    assert_error(
        status,
        &body,
        StatusCode::FORBIDDEN,
        "password_confirmation_required",
    );

    // 新しいパスワードで確かめ直せば通る。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/auth/confirm-password",
        &alice,
        r#"{"password":"new-password"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, body) = send_empty(app, "POST", "/api/v1/auth/recovery-code", &alice).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// 自分でパスワードを変えたセッションは確かめた内のままで、同じ人のほかのセッションは通らなくなる。
#[sqlx::test]
async fn changing_the_own_password_keeps_only_this_session_confirmed(pool: SqlitePool) {
    insert_user(&pool, "alice", "old-password").await;
    let app = test_app(pool).await;
    let here = login(app.clone(), "alice", "old-password").await;
    let elsewhere = login(app.clone(), "alice", "old-password").await;

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        "/api/v1/auth/password",
        &here,
        r#"{"newPassword":"new-password"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let rename = r#"{"username":"alice2"}"#;
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        "/api/v1/auth/username",
        &elsewhere,
        rename,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::FORBIDDEN,
        "password_confirmation_required",
    );
    let (status, body) = send_json(app, "PUT", "/api/v1/auth/username", &here, rename).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
}

/// 管理者だけの書き換えは、確かめた内でなければ断る。バックアップは、落とす口でなく引換券を出す口で断る。
#[sqlx::test]
async fn admin_only_writes_require_a_fresh_confirmation(pool: SqlitePool) {
    insert_admin(&pool, "admin", "old-password").await;
    let app = test_app(pool.clone()).await;
    let cookie = login(app.clone(), "admin", "old-password").await;
    replace_password_hash(&pool, "admin", "new-password").await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/admin/users",
        &cookie,
        r#"{"username":"newbie","password":"pw","role":"user"}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::FORBIDDEN,
        "password_confirmation_required",
    );

    let (status, body) =
        send_empty(app, "POST", "/api/v1/admin/backup/ticket", &cookie).await;
    assert_error(
        status,
        &body,
        StatusCode::FORBIDDEN,
        "password_confirmation_required",
    );
}

/// 管理者が自分をユーザー管理から変えようとしたら、確かめた内でなくても、確かめ直しより先に `forbidden` で断る。
#[sqlx::test]
async fn changing_oneself_from_user_management_is_forbidden_before_asking_the_password(
    pool: SqlitePool,
) {
    insert_admin(&pool, "admin", "old-password").await;
    let app = test_app(pool.clone()).await;
    let cookie = login(app.clone(), "admin", "old-password").await;
    let admin_id = db_user_id(&pool, "admin").await;
    replace_password_hash(&pool, "admin", "new-password").await;

    let (status, body) = send_json(
        app,
        "PUT",
        &format!("/api/v1/admin/users/{admin_id}/role"),
        &cookie,
        r#"{"role":"user"}"#,
    )
    .await;
    assert_error(status, &body, StatusCode::FORBIDDEN, "forbidden");
}

/// 管理者が他人のコンテンツを書き換えるときは確かめ直しを求め、自分のものには求めない。
#[sqlx::test]
async fn editing_others_contents_requires_a_fresh_confirmation_but_own_does_not(pool: SqlitePool) {
    insert_admin(&pool, "admin", "old-password").await;
    insert_user(&pool, "alice", "alice-password").await;
    let admin_id = db_user_id(&pool, "admin").await;
    let alice_id = db_user_id(&pool, "alice").await;
    let own = insert_link_by(&pool, "自分の", "public", Some(admin_id), None).await;
    let others = insert_link_by(&pool, "他人の", "public", Some(alice_id), None).await;
    let app = test_app(pool.clone()).await;
    let cookie = login(app.clone(), "admin", "old-password").await;
    replace_password_hash(&pool, "admin", "new-password").await;

    let edit = r#"{"title":"改題","url":"https://example.com"}"#;
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{others}"),
        &cookie,
        edit,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::FORBIDDEN,
        "password_confirmation_required",
    );

    let (status, body) = send_json(
        app,
        "PUT",
        &format!("/api/v1/contents/{own}"),
        &cookie,
        edit,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
