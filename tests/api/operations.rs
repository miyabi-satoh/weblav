//! 操作の記録 (→ docs/operations.md)。何を記録するかは各操作の口で1件ずつ確かめず、
//! 記録が入口につながっていることと、一覧の読み方 (並び・続き・絞り込み) を確かめる。

use super::*;

async fn list(app: Router, cookie: &str, query: &str) -> serde_json::Value {
    let uri = format!("/api/v1/admin/operations?{query}");
    let (status, body) = send_empty(app, "GET", &uri, cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    parse_json(&body)
}

fn ops(body: &serde_json::Value) -> Vec<&str> {
    body["items"]
        .as_array()
        .expect("配列のはず")
        .iter()
        .map(|item| item["op"].as_str().expect("op"))
        .collect()
}

/// 記録は新しい順に返し、続きは `nextBefore` から読む。
#[sqlx::test]
async fn operations_are_listed_newest_first_in_pages(pool: SqlitePool) {
    let (app, admin) = admin_app(&pool).await;
    for (op, path) in [("mkdir", "a"), ("upload", "a/b.pdf"), ("delete", "c")] {
        sqlx::query(
            "INSERT INTO operations (category, user_id, username, op, root_id, root_name, paths, result)
             VALUES ('shares', 1, 'admin', ?, 1, '教材', json_array(?), 'succeeded')",
        )
        .bind(op)
        .bind(path)
        .execute(&pool)
        .await
        .expect("記録を入れられなかった");
    }

    let body = list(app.clone(), &admin, "category=shares&limit=2").await;
    assert_eq!(ops(&body), vec!["delete", "upload"]);
    assert_eq!(body["items"][1]["paths"], serde_json::json!(["a/b.pdf"]));
    assert_eq!(body["items"][1]["shareName"], "教材");
    let next = body["nextBefore"].as_i64().expect("続きがあるはず");

    let body = list(
        app,
        &admin,
        &format!("category=shares&limit=2&before={next}"),
    )
    .await;
    assert_eq!(ops(&body), vec!["mkdir"]);
    assert!(body["nextBefore"].is_null(), "{body}");
}

/// ログインは失敗も残す。打ち込まれた名前のユーザーがいれば、その人で絞り込める。
#[sqlx::test]
async fn failed_and_successful_logins_are_recorded_and_filterable(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let (app, admin) = admin_app(&pool).await;
    let alice = db_user_id(&pool, "alice").await;
    for username in ["alice", "nobody"] {
        let (status, _) = send_anon_json(
            app.clone(),
            "POST",
            "/api/v1/auth/login",
            &format!(r#"{{"username":"{username}","password":"wrong"}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    login(app.clone(), "alice", "correct-password").await;

    let body = list(app.clone(), &admin, &format!("userId={alice}")).await;
    let items = body["items"].as_array().expect("配列のはず");
    let results: Vec<(&str, &str)> = items
        .iter()
        .map(|item| {
            (
                item["op"].as_str().expect("op"),
                item["result"].as_str().expect("result"),
            )
        })
        .collect();
    assert_eq!(results, vec![("login", "succeeded"), ("login", "failed")]);
    assert_eq!(items[1]["error"], "invalid_credentials");

    // いない名前の失敗は、打ち込まれた名前で残り、ユーザーの id を持たない。
    let body = list(app, &admin, "category=auth").await;
    let nobody = body["items"]
        .as_array()
        .expect("配列のはず")
        .iter()
        .find(|item| item["username"] == "nobody")
        .expect("いない名前の失敗が残るはず");
    assert!(nobody["userId"].is_null(), "{nobody}");
}

/// 編集は変えた項目を残し (短い値は前と後の値も)、何も変えずに保存したものは残さない。
#[sqlx::test]
async fn editing_records_only_the_changed_fields(pool: SqlitePool) {
    let (app, admin) = admin_app(&pool).await;
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/contents",
        &admin,
        r#"{"type":"group","title":"時間割","visibility":"public"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = extract_id(&body);
    for title in ["時間割(改)", "時間割(改)"] {
        let (status, body) = send_json(
            app.clone(),
            "PUT",
            &format!("/api/v1/contents/{id}"),
            &admin,
            &format!(r#"{{"title":"{title}","visibility":"authenticated"}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    let body = list(app, &admin, "category=contents").await;
    assert_eq!(ops(&body), vec!["update_content", "create_content"]);
    let updated = &body["items"][0];
    assert_eq!(updated["targetId"], id);
    // 変える前の名前で残す。
    assert_eq!(updated["targetName"], "時間割");
    assert_eq!(
        updated["fields"],
        serde_json::json!([
            { "name": "title", "values": { "before": "時間割", "after": "時間割(改)" } },
            { "name": "visibility", "values": { "before": "public", "after": "authenticated" } }
        ])
    );
}
