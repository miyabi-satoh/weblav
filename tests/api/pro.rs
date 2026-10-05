//! Free の上限と Pro (→ docs/pro.md)。

use super::*;

/// Free の上限に当たった 409 の本文。
fn assert_free_limit(status: StatusCode, body: &str, target: &str, limit: i64) {
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let json = parse_json(body);
    assert_eq!(json["error"]["code"], "free_limit_reached", "{json}");
    assert_eq!(
        json["error"]["detail"],
        serde_json::json!({ "kind": "freeLimit", "target": target, "limit": limit }),
    );
}

/// Free の上限は種類ごとに WebLAV 全体で数え、作成者や公開範囲を問わない (→ docs/pro.md「上限を数えて止める」)。
#[sqlx::test]
async fn free_rejects_contents_over_the_limit(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    insert_user(&pool, "bob", "password").await;
    let bob = db_user_id(&pool, "bob").await;
    for n in 0..10 {
        insert_link_by(&pool, &format!("リンク{n}"), "private", Some(bob), None).await;
    }
    let dir = temp_test_dir("free-limit-archive");
    for n in 0..3 {
        insert_archive_by(&pool, &format!("アーカイブ{n}"), &dir, "public", None, None).await;
    }

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/contents",
        &cookie,
        r#"{"type":"link","title":"リンク","url":"https://example.com/"}"#,
    )
    .await;
    assert_free_limit(status, &body, "link", 10);

    let path_json = json_string(&dir.display().to_string());
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/contents",
        &cookie,
        &format!(r#"{{"type":"archive","path":{path_json}}}"#),
    )
    .await;
    assert_free_limit(status, &body, "archive", 3);

    // 上限の無い種類と、まだ上限に届いていない種類は足せる。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/contents",
        &cookie,
        r#"{"type":"group","title":"グループ"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/contents",
        &cookie,
        &format!(r#"{{"type":"folder","path":{path_json}}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[sqlx::test]
async fn free_rejects_uploads_over_the_limit(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    for n in 0..10 {
        sqlx::query(
            "INSERT INTO contents (type, title, blob_hash, file_name, file_size) \
             VALUES ('file', ?, 'hash', 'a.txt', 1)",
        )
        .bind(format!("ファイル{n}"))
        .execute(&pool)
        .await
        .expect("fileを挿入できなかった");
    }

    let (status, body) = send(
        app,
        multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[],
            Some(("file", "doc.txt", b"hello")),
        ),
    )
    .await;
    assert_free_limit(status, &body, "file", 10);
}

/// 上限を超えていても、あるものは直せる。止めるのは足すことだけ (→ docs/pro.md「上限を数えて止める」)。
#[sqlx::test]
async fn free_keeps_contents_over_the_limit_editable(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    let dir = temp_test_dir("free-limit-over");
    let mut ids = Vec::new();
    for n in 0..4 {
        ids.push(
            insert_archive_by(&pool, &format!("アーカイブ{n}"), &dir, "public", None, None).await,
        );
    }

    let path_json = json_string(&dir.display().to_string());
    let (status, body) = send_json(
        app,
        "PUT",
        &format!("/api/v1/contents/{}", ids[3]),
        &cookie,
        &format!(r#"{{"title":"直したアーカイブ","path":{path_json}}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// 管理者は1人・編集者は3人まで。作るときも、ロールを変えるときも数える。
#[sqlx::test]
async fn free_rejects_users_over_the_limit(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    for name in ["bob", "carol", "dave"] {
        insert_user(&pool, name, "password").await;
    }

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/admin/users",
        &cookie,
        r#"{"username":"erin","password":"password"}"#,
    )
    .await;
    assert_free_limit(status, &body, "user", 3);

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/admin/users",
        &cookie,
        r#"{"username":"erin","password":"password","role":"admin"}"#,
    )
    .await;
    assert_free_limit(status, &body, "admin", 1);

    let bob = user_id(app.clone(), &cookie, "bob").await;
    let (status, body) = send_json(
        app,
        "PUT",
        &format!("/api/v1/admin/users/{bob}/role"),
        &cookie,
        r#"{"role":"admin"}"#,
    )
    .await;
    assert_free_limit(status, &body, "admin", 1);
}

#[sqlx::test]
async fn pro_lifts_the_limits(pool: SqlitePool) {
    insert_admin(&pool, "admin-alice", "correct-password").await;
    let app = test_app_pro(pool.clone()).await;
    let cookie = login(app.clone(), "admin-alice", "correct-password").await;
    let dir = temp_test_dir("pro-archive");
    for n in 0..3 {
        insert_archive_by(&pool, &format!("アーカイブ{n}"), &dir, "public", None, None).await;
    }

    let path_json = json_string(&dir.display().to_string());
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/contents",
        &cookie,
        &format!(r#"{{"type":"archive","path":{path_json}}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/admin/users",
        &cookie,
        r#"{"username":"bob","password":"password","role":"admin"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// 確かめられない証明は、無いのと同じ (Free で動く)。
#[sqlx::test]
async fn invalid_pro_proof_runs_as_free(pool: SqlitePool) {
    insert_admin(&pool, "admin-alice", "correct-password").await;
    let mut file: serde_json::Value =
        serde_json::from_str(DEV_PRO).expect("開発用の pro.json を読めなかった");
    let proof = file["binding"]["permission"]["value"]
        .as_str()
        .expect("開発用の pro.json に証明が無い");
    let (payload, _) = proof.split_once('.').expect("証明に区切りの . が無い");
    file["binding"]["permission"]["value"] = format!("{payload}.AAAA").into();
    let app = test_app_with_pro_file(pool, &file.to_string()).await;
    let cookie = login(app.clone(), "admin-alice", "correct-password").await;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/admin/users",
        &cookie,
        r#"{"username":"bob","password":"password","role":"admin"}"#,
    )
    .await;
    assert_free_limit(status, &body, "admin", 1);
}

async fn get_pro(app: Router, cookie: &str) -> (StatusCode, serde_json::Value) {
    let (status, body) = send_empty(app, "GET", "/api/v1/admin/pro", cookie).await;
    let json = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
    (status, json)
}

/// Free の区画には、上限のある対象ごとの件数と上限を出す (→ docs/pro.md「上限を数えて止める」)。
#[sqlx::test]
async fn pro_status_shows_usage_on_free(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    insert_user(&pool, "bob", "password").await;
    insert_link_by(&pool, "リンク", "public", None, None).await;

    let (status, json) = get_pro(app, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["edition"], "free");
    assert_eq!(json["plan"], serde_json::Value::Null);
    let usage = |target: &str| {
        json["usage"]
            .as_array()
            .expect("usage が配列でない")
            .iter()
            .find(|u| u["target"] == target)
            .cloned()
            .expect("usage に対象が無い")
    };
    assert_eq!(
        usage("admin"),
        serde_json::json!({ "target": "admin", "count": 1, "limit": 1 })
    );
    assert_eq!(
        usage("user"),
        serde_json::json!({ "target": "user", "count": 1, "limit": 3 })
    );
    assert_eq!(
        usage("link"),
        serde_json::json!({ "target": "link", "count": 1, "limit": 10 })
    );
    assert_eq!(usage("archive")["count"], 0);
}

/// アカウントから外すと、起動し直さずに Free の上限が効く。
#[sqlx::test]
async fn removing_pro_turns_the_limits_on(pool: SqlitePool) {
    insert_admin(&pool, "admin-alice", "correct-password").await;
    let app = test_app_pro(pool.clone()).await;
    let cookie = login(app.clone(), "admin-alice", "correct-password").await;
    let (_, json) = get_pro(app.clone(), &cookie).await;
    assert_eq!(json["edition"], "pro");
    assert_eq!(json["plan"], "personal");

    let (status, body) = send_empty(app.clone(), "DELETE", "/api/v1/admin/pro", &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let json = parse_json(&body);
    assert_eq!(json["pro"]["edition"], "free");

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/admin/users",
        &cookie,
        r#"{"username":"bob","password":"password","role":"admin"}"#,
    )
    .await;
    assert_free_limit(status, &body, "admin", 1);
}

/// 結びかけると窓口の結ぶ画面の URL が出て、「やめる」で消える。
#[sqlx::test]
async fn link_can_be_started_and_cancelled(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_empty(app.clone(), "POST", "/api/v1/admin/pro/link", &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let json = parse_json(&body);
    let url = json["linkUrl"].as_str().expect("linkUrl が無い");
    assert!(url.contains("/account/link?r="), "{url}");
    assert_eq!(json["edition"], "free");

    let (status, body) = send_empty(app, "DELETE", "/api/v1/admin/pro/link", &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse_json(&body)["linkUrl"], serde_json::Value::Null);
}

/// 打ち間違えた返しのコードは 422 で断り、Free のまま。
#[sqlx::test]
async fn wrong_code_is_rejected(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    send_empty(app.clone(), "POST", "/api/v1/admin/pro/link", &cookie).await;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/admin/pro/link/code",
        &cookie,
        r#"{"code":"AAAAA-AAAAA-AAAAA"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(parse_json(&body)["error"]["code"], "pro_code_invalid");
}

/// 結びかけていないのに返しのコードを打ち込んでも、受け取らない。
#[sqlx::test]
async fn code_without_link_is_conflict(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/admin/pro/link/code",
        &cookie,
        r#"{"code":"AAAAA-AAAAA-AAAAA"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}
