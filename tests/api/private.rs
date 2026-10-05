//! 本人のみ・非表示 (docs/access.md「公開範囲と匿名閲覧」・「親が外れるときの公開範囲」・「権限」)

use super::*;

/// APIで作ったコンテンツには作成者が記録され、応答と管理画面の一覧に作成者名が出る。
#[sqlx::test]
async fn content_created_via_api_records_its_creator(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/contents",
        &cookie,
        r#"{"type":"link","title":"社内Wiki","url":"https://example.com"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(body.contains(r#""createdByUsername":"alice""#), "{body}");

    let (status, body) = send_empty(app, "GET", "/api/v1/admin/contents", &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""createdByUsername":"alice""#), "{body}");
}

/// 閲覧の場の一覧には、`private` は作成者にだけ並び、`hidden` は誰にも並ばない。
#[sqlx::test]
async fn contents_list_shows_private_only_to_its_creator_and_never_hidden(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    insert_user(&pool, "bob", "correct-password").await;
    let alice_id = db_user_id(&pool, "alice").await;
    insert_link_by(&pool, "アリスのメモ", "private", Some(alice_id), None).await;
    insert_link_by(&pool, "準備中の資料", "hidden", Some(alice_id), None).await;
    let app = test_app(pool).await;

    let alice_cookie = login(app.clone(), "alice", "correct-password").await;
    let (status, body) = send_empty(app.clone(), "GET", "/api/v1/contents", &alice_cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("アリスのメモ"), "{body}");
    assert!(body.contains(r#""private":true"#), "{body}");
    assert!(!body.contains("準備中の資料"), "{body}");

    let bob_cookie = login(app.clone(), "bob", "correct-password").await;
    let (_, body) = send_empty(app.clone(), "GET", "/api/v1/contents", &bob_cookie).await;
    assert!(!body.contains("アリスのメモ"), "{body}");
    assert!(!body.contains("準備中の資料"), "{body}");

    let (_, body) = send_anon(app, "GET", "/api/v1/contents").await;
    assert!(!body.contains("アリスのメモ"), "{body}");
}

/// 他人の `private` を開くと、匿名には401、ログイン済みには404を返す。
/// 作成者と `admin` は開ける (→ docs/access.md「匿名閲覧の受け口」)。
#[sqlx::test]
async fn opening_private_content_depends_on_the_viewer(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    insert_user(&pool, "bob", "correct-password").await;
    let alice_id = db_user_id(&pool, "alice").await;
    let dir = temp_test_dir("open-private");
    let content_id = insert_folder_by(
        &pool,
        "私の教材",
        dir.display().to_string(),
        "private",
        Some(alice_id),
        None,
    )
    .await;
    let app = test_app(pool.clone()).await;
    let uri = format!("/api/v1/contents/{content_id}/browse");

    let (status, _) = send_anon(app.clone(), "GET", &uri).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let bob_cookie = login(app.clone(), "bob", "correct-password").await;
    let (status, _) = send_empty(app.clone(), "GET", &uri, &bob_cookie).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let alice_cookie = login(app.clone(), "alice", "correct-password").await;
    let (status, body) = send_empty(app.clone(), "GET", &uri, &alice_cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let admin_cookie = admin_cookie(app.clone(), &pool).await;
    let (status, body) = send_empty(app, "GET", &uri, &admin_cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// `hidden` のグループはログイン済みなら開けて、中の `hidden` も並ぶ。
/// 閲覧の場の一覧には出ない (→ docs/access.md「匿名閲覧の受け口」)。
#[sqlx::test]
async fn hidden_group_opens_for_logged_in_users_with_its_children(pool: SqlitePool) {
    insert_user(&pool, "bob", "correct-password").await;
    let group_id = insert_group_under(&pool, "準備中のグループ", "hidden", None).await;
    insert_link_by(&pool, "準備中の資料", "hidden", None, Some(group_id)).await;
    let app = test_app(pool).await;
    let uri = format!("/api/v1/contents/{group_id}/group");

    let (status, _) = send_anon(app.clone(), "GET", &uri).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let cookie = login(app.clone(), "bob", "correct-password").await;
    let (status, body) = send_empty(app.clone(), "GET", &uri, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("準備中の資料"), "{body}");

    let (_, body) = send_empty(app, "GET", "/api/v1/contents", &cookie).await;
    assert!(!body.contains("準備中のグループ"), "{body}");
}

/// グループは `private` にできない。作成でも更新でも、誰の操作でも422 (→ docs/access.md「ロールと操作」)。
/// 更新では、作成者でない人の要求でも権限エラー (403) より先に422を返す。
#[sqlx::test]
async fn group_cannot_be_private(pool: SqlitePool) {
    let group_id = insert_group_under(&pool, "共有の資料", "public", None).await;
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/contents",
        &cookie,
        r#"{"type":"group","title":"私の置き場","visibility":"private"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let (status, body) = send_json(
        app,
        "PUT",
        &format!("/api/v1/contents/{group_id}"),
        &cookie,
        r#"{"title":"共有の資料","visibility":"private"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

/// 管理画面の一覧で、他人の `private` は `user` には出ず、`admin` には作成者名付きで出る。
/// `hidden` は全員に出る (→ docs/access.md「管理画面の一覧が `user` に見えること」)。
#[sqlx::test]
async fn admin_contents_list_shows_others_private_only_to_admins(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    insert_user(&pool, "bob", "correct-password").await;
    let alice_id = db_user_id(&pool, "alice").await;
    insert_link_by(&pool, "アリスのメモ", "private", Some(alice_id), None).await;
    insert_link_by(&pool, "準備中の資料", "hidden", None, None).await;
    let app = test_app(pool.clone()).await;

    let bob_cookie = login(app.clone(), "bob", "correct-password").await;
    let (status, body) =
        send_empty(app.clone(), "GET", "/api/v1/admin/contents", &bob_cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains("アリスのメモ"), "{body}");
    assert!(body.contains("準備中の資料"), "{body}");

    let alice_cookie = login(app.clone(), "alice", "correct-password").await;
    let (_, body) = send_empty(app.clone(), "GET", "/api/v1/admin/contents", &alice_cookie).await;
    assert!(body.contains("アリスのメモ"), "{body}");

    let admin_cookie = admin_cookie(app.clone(), &pool).await;
    let (_, body) = send_empty(app, "GET", "/api/v1/admin/contents", &admin_cookie).await;
    assert!(body.contains("アリスのメモ"), "{body}");
    assert!(body.contains(r#""createdByUsername":"alice""#), "{body}");
}

/// 他人の `private` のアーカイブは、ログイン済みでも作成者と `admin` 以外には404。
#[sqlx::test]
async fn private_archive_view_is_not_found_for_other_logged_in_users(pool: SqlitePool) {
    insert_admin(&pool, "admin", "correct-password").await;
    insert_user(&pool, "bob", "correct-password").await;
    let admin_id = db_user_id(&pool, "admin").await;
    let dir = temp_test_dir("private-archive");
    let archive_id = insert_archive_by(
        &pool,
        "管理者の索引",
        dir.display().to_string(),
        "private",
        Some(admin_id),
        None,
    )
    .await;
    let app = test_app(pool).await;
    let uri = format!("/api/v1/contents/{archive_id}/archive");

    let (status, _) = send_anon(app.clone(), "GET", &uri).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let bob_cookie = login(app.clone(), "bob", "correct-password").await;
    let (status, _) = send_empty(app.clone(), "GET", &uri, &bob_cookie).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let admin_cookie = login(app.clone(), "admin", "correct-password").await;
    let (status, body) = send_empty(app, "GET", &uri, &admin_cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// ユーザーを削除すると、その人の `private` は `hidden` になって残り、作成者の記録が消える
/// (→ docs/access.md「ユーザーの削除と作成者」)。
#[sqlx::test]
async fn deleting_user_turns_their_private_contents_hidden(pool: SqlitePool) {
    insert_user(&pool, "bob", "correct-password").await;
    let bob_id = db_user_id(&pool, "bob").await;
    let private_id = insert_link_by(&pool, "ボブのメモ", "private", Some(bob_id), None).await;
    let public_id = insert_link_by(&pool, "ボブのリンク", "public", Some(bob_id), None).await;
    let db = pool.clone();
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_empty(
        app,
        "DELETE",
        &format!("/api/v1/admin/users/{bob_id}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let row = |id: i64| {
        sqlx::query_as::<_, (String, Option<i64>)>(
            "SELECT visibility, created_by FROM contents WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&db)
    };
    let private_row = row(private_id).await.expect("メモが残っていない");
    assert_eq!(private_row, ("hidden".to_string(), None));
    let public_row = row(public_id).await.expect("リンクが残っていない");
    assert_eq!(public_row, ("public".to_string(), None));
}

/// `private` にできるのは作成者本人だけ。`admin` も他人のものは `private` にできない
/// (→ docs/access.md「ロールと操作」)。
#[sqlx::test]
async fn only_the_creator_can_make_content_private(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    insert_user(&pool, "bob", "correct-password").await;
    let alice_id = db_user_id(&pool, "alice").await;
    let link_id = insert_link_by(&pool, "アリスのリンク", "public", Some(alice_id), None).await;
    let app = test_app(pool.clone()).await;
    let uri = format!("/api/v1/contents/{link_id}");
    let to_private =
        r#"{"title":"アリスのリンク","url":"https://example.com","visibility":"private"}"#;

    let bob_cookie = login(app.clone(), "bob", "correct-password").await;
    let (status, body) = send_json(app.clone(), "PUT", &uri, &bob_cookie, to_private).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let admin_cookie = admin_cookie(app.clone(), &pool).await;
    let (status, body) = send_json(app.clone(), "PUT", &uri, &admin_cookie, to_private).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let alice_cookie = login(app.clone(), "alice", "correct-password").await;
    let (status, body) = send_json(app, "PUT", &uri, &alice_cookie, to_private).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""visibility":"private""#), "{body}");
}

/// `admin` は他人の `private` を `hidden` にだけ変えられる。値を変えない更新は通る
/// (→ docs/access.md「ロールと操作」)。
#[sqlx::test]
async fn admin_can_change_others_private_only_to_hidden(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let alice_id = db_user_id(&pool, "alice").await;
    let link_id = insert_link_by(&pool, "アリスのメモ", "private", Some(alice_id), None).await;
    let app = test_app(pool.clone()).await;
    let uri = format!("/api/v1/contents/{link_id}");
    let cookie = admin_cookie(app.clone(), &pool).await;
    let update = |visibility: &str| {
        format!(
            r#"{{"title":"アリスのメモ","url":"https://example.com","visibility":"{visibility}"}}"#
        )
    };

    let (status, body) = send_json(app.clone(), "PUT", &uri, &cookie, &update("public")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let (status, body) = send_json(app.clone(), "PUT", &uri, &cookie, &update("private")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""visibility":"private""#), "{body}");

    let (status, body) = send_json(app, "PUT", &uri, &cookie, &update("hidden")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""visibility":"hidden""#), "{body}");
}

/// 管理用の操作で、他人の `private` は `user` には存在しないものとして404になる
/// (→ docs/access.md「ロールと操作」)。
#[sqlx::test]
async fn management_endpoints_hide_others_private_from_regular_users(pool: SqlitePool) {
    insert_admin(&pool, "admin", "correct-password").await;
    insert_user(&pool, "alice", "correct-password").await;
    insert_user(&pool, "bob", "correct-password").await;
    let alice_id = db_user_id(&pool, "alice").await;
    let admin_id = db_user_id(&pool, "admin").await;
    let link_id = insert_link_by(&pool, "アリスのメモ", "private", Some(alice_id), None).await;
    let dir = temp_test_dir("manage-private-archive");
    let archive_id = insert_archive_by(
        &pool,
        "管理者の索引",
        dir.display().to_string(),
        "private",
        Some(admin_id),
        None,
    )
    .await;
    let axis_id: i64 = sqlx::query_scalar(
        "INSERT INTO archive_axes (archive_id, name, source, position) \
         VALUES (?, '科目', 'filename_word', 0) RETURNING id",
    )
    .bind(archive_id)
    .fetch_one(&pool)
    .await
    .expect("failed to insert axis");
    let item_id: i64 = sqlx::query_scalar(
        "INSERT INTO archive_items (archive_id, rel_path) VALUES (?, 'a.pdf') RETURNING id",
    )
    .bind(archive_id)
    .fetch_one(&pool)
    .await
    .expect("failed to insert item");
    sqlx::query(
        "INSERT INTO archive_axis_values (axis_id, raw_value, position) VALUES (?, '国語', 0)",
    )
    .bind(axis_id)
    .execute(&pool)
    .await
    .expect("failed to insert axis value");
    let db = pool.clone();
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "bob", "correct-password").await;

    // タイトルが空でも、入力の検証 (422) より先に存在を隠す404が返る。
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{link_id}"),
        &cookie,
        r#"{"title":"","url":"https://example.com"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = send_empty(
        app.clone(),
        "DELETE",
        &format!("/api/v1/contents/{link_id}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // link に対する差し替えは本来422 (ファイル型ではない) だが、存在を隠す404が先に返る。
    let response = app
        .clone()
        .oneshot(multipart_request(
            "PUT",
            &format!("/api/v1/contents/{link_id}/upload"),
            &cookie,
            &[("title", "改題")],
            None,
        ))
        .await
        .expect("request should not fail");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    for (method, path) in [
        ("POST", format!("/api/v1/contents/{archive_id}/rescan")),
        ("GET", format!("/api/v1/contents/{archive_id}/items")),
        (
            "GET",
            format!("/api/v1/contents/{archive_id}/items/{item_id}/manage-download"),
        ),
        ("GET", format!("/api/v1/contents/{archive_id}/axes")),
        (
            "GET",
            format!("/api/v1/contents/{archive_id}/axes/{axis_id}/values"),
        ),
    ] {
        let (status, body) = send_empty(app.clone(), method, &path, &cookie).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}: {body}");
    }

    // 書き込み系も、有効な要求に対して404を返し、何も書き込まない。
    for (path, json) in [
        (
            format!("/api/v1/contents/{archive_id}/items"),
            format!(r#"{{"itemIds":[{item_id}],"published":true}}"#),
        ),
        (
            format!("/api/v1/contents/{archive_id}/items/{item_id}"),
            r#"{"published":true}"#.to_string(),
        ),
        (
            format!("/api/v1/contents/{archive_id}/axes/{axis_id}/values"),
            r#"{"values":[{"rawValue":"英語"}]}"#.to_string(),
        ),
    ] {
        let (status, body) = send_json(app.clone(), "PUT", &path, &cookie, &json).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "PUT {path}: {body}");
    }
    let published: i64 = sqlx::query_scalar("SELECT published FROM archive_items WHERE id = ?")
        .bind(item_id)
        .fetch_one(&db)
        .await
        .expect("アイテムを取得できなかった");
    assert_eq!(published, 0, "他人の private のアイテムが公開された");
    let values: Vec<String> =
        sqlx::query_scalar("SELECT raw_value FROM archive_axis_values WHERE axis_id = ?")
            .bind(axis_id)
            .fetch_all(&db)
            .await
            .expect("値の辞書を取得できなかった");
    assert_eq!(
        values,
        vec!["国語".to_string()],
        "他人の private の値の辞書が書き換わった"
    );
}

/// 他人の (private でない) アーカイブは、一般ユーザーから見えるが書き換えられない
/// (→ docs/access.md「ロールと操作」)。読む操作は通り、書き換える操作は403で何も書き込まない。
#[sqlx::test]
async fn archive_endpoints_forbid_writes_to_others_archives(pool: SqlitePool) {
    insert_admin(&pool, "admin", "correct-password").await;
    insert_user(&pool, "bob", "correct-password").await;
    let admin_id = db_user_id(&pool, "admin").await;
    let dir = temp_test_dir("manage-others-archive");
    let archive_id = insert_archive_by(
        &pool,
        "管理者の索引",
        dir.display().to_string(),
        "public",
        Some(admin_id),
        None,
    )
    .await;
    let axis_id: i64 = sqlx::query_scalar(
        "INSERT INTO archive_axes (archive_id, name, source, position) \
         VALUES (?, '科目', 'filename_word', 0) RETURNING id",
    )
    .bind(archive_id)
    .fetch_one(&pool)
    .await
    .expect("failed to insert axis");
    let item_id: i64 = sqlx::query_scalar(
        "INSERT INTO archive_items (archive_id, rel_path) VALUES (?, 'a.pdf') RETURNING id",
    )
    .bind(archive_id)
    .fetch_one(&pool)
    .await
    .expect("failed to insert item");
    let db = pool.clone();
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "bob", "correct-password").await;

    for path in [
        format!("/api/v1/contents/{archive_id}/items"),
        format!("/api/v1/contents/{archive_id}/axes"),
        format!("/api/v1/contents/{archive_id}/axes/{axis_id}/values"),
    ] {
        let (status, body) = send_empty(app.clone(), "GET", &path, &cookie).await;
        assert_eq!(status, StatusCode::OK, "GET {path}: {body}");
    }

    let (status, body) = send_empty(
        app.clone(),
        "POST",
        &format!("/api/v1/contents/{archive_id}/rescan"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, body) = send_empty(
        app.clone(),
        "DELETE",
        &format!("/api/v1/contents/{archive_id}/axes/{axis_id}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    for (path, json) in [
        (
            format!("/api/v1/contents/{archive_id}/items"),
            format!(r#"{{"itemIds":[{item_id}],"published":true}}"#),
        ),
        (
            format!("/api/v1/contents/{archive_id}/items/{item_id}"),
            r#"{"published":true}"#.to_string(),
        ),
        (
            format!("/api/v1/contents/{archive_id}/axes/{axis_id}"),
            r#"{"name":"教科","source":"filenameWord","position":0}"#.to_string(),
        ),
        (
            format!("/api/v1/contents/{archive_id}/axes/{axis_id}/values"),
            r#"{"values":[{"rawValue":"英語"}]}"#.to_string(),
        ),
    ] {
        let (status, body) = send_json(app.clone(), "PUT", &path, &cookie, &json).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "PUT {path}: {body}");
    }

    let published: i64 = sqlx::query_scalar("SELECT published FROM archive_items WHERE id = ?")
        .bind(item_id)
        .fetch_one(&db)
        .await
        .expect("アイテムを取得できなかった");
    assert_eq!(published, 0, "他人のアイテムが公開された");
    let name: String = sqlx::query_scalar("SELECT name FROM archive_axes WHERE id = ?")
        .bind(axis_id)
        .fetch_one(&db)
        .await
        .expect("軸を取得できなかった");
    assert_eq!(name, "科目", "他人の軸が書き換わった");
}

/// 自分のコンテンツは、他人のグループにも入れられる (→ docs/access.md「ロールと操作」)。
/// 書き換わるのは入れる側 (子) の行だけのため。
#[sqlx::test]
async fn regular_user_can_put_own_content_into_others_group(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let group_id = insert_group_under(&pool, "英検", "public", None).await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/contents",
        &cookie,
        r#"{"type":"link","title":"リンク","url":"https://example.com"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id: serde_json::Value = serde_json::from_str(&body).expect("JSON");
    let id = id["id"].as_i64().expect("id");

    let (status, body) = send_json(
        app,
        "PUT",
        &format!("/api/v1/contents/{id}"),
        &cookie,
        &format!(r#"{{"title":"リンク","url":"https://example.com","parentId":{group_id}}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains(&format!(r#""parentId":{group_id}"#)),
        "{body}"
    );
}

/// 作成者は `admin` だけが付け替えられる。付け替えた先の `user` は書き換えられるようになる。
/// `private` のものは付け替えられず、いないユーザーは422 (→ docs/access.md「ロールと操作」)。
#[sqlx::test]
async fn created_by_can_be_changed_only_by_an_admin(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    insert_user(&pool, "bob", "correct-password").await;
    let alice_id = db_user_id(&pool, "alice").await;
    let bob_id = db_user_id(&pool, "bob").await;
    let link_id = insert_link_by(&pool, "社内Wiki", "public", None, None).await;
    let private_id = insert_link_by(&pool, "メモ", "private", Some(alice_id), None).await;
    let app = test_app(pool.clone()).await;
    let admin_cookie = admin_cookie(app.clone(), &pool).await;
    let bob_cookie = login(app.clone(), "bob", "correct-password").await;
    let put = |cookie: String, id: i64, created_by: i64| {
        let app = app.clone();
        async move {
            send_json(
                app,
                "PUT",
                &format!("/api/v1/contents/{id}/creator"),
                &cookie,
                &format!(r#"{{"createdBy":{created_by}}}"#),
            )
            .await
        }
    };

    let (status, body) = put(admin_cookie.clone(), link_id, bob_id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""createdByUsername":"bob""#), "{body}");

    // 付け替えた先の本人は書き換えられるが、ほかへ付け替えることはできない。
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{link_id}"),
        &bob_cookie,
        r#"{"title":"改題","url":"https://example.com"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = put(bob_cookie, link_id, alice_id).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let (status, body) = put(admin_cookie.clone(), private_id, bob_id).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let (status, body) = put(admin_cookie, link_id, 9999).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

/// 一般ユーザーも、作成時に `private` を選べる (作成者は本人のため → docs/access.md「ロールと操作」)。
/// JSON と multipart の両方の経路で確かめる。
#[sqlx::test]
async fn regular_user_can_create_private_content(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let (app, _blobs_dir) = test_app_with_blobs(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/contents",
        &cookie,
        r#"{"type":"link","title":"私のリンク","url":"https://example.com","visibility":"private"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(body.contains(r#""visibility":"private""#), "{body}");

    let response = app
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "私の資料"), ("visibility", "private")],
            Some(("file", "doc.txt", b"hello")),
        ))
        .await
        .expect("request should not fail");
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = body_string(response).await;
    assert!(body.contains(r#""visibility":"private""#), "{body}");
}

/// ユーザー一覧に、そのユーザーが作成者の `private` の件数が載る (→ docs/access.md「ユーザーの削除と作成者」)。
#[sqlx::test]
async fn users_list_includes_private_content_count(pool: SqlitePool) {
    insert_user(&pool, "bob", "correct-password").await;
    let bob_id = db_user_id(&pool, "bob").await;
    insert_link_by(&pool, "メモ1", "private", Some(bob_id), None).await;
    insert_link_by(&pool, "メモ2", "private", Some(bob_id), None).await;
    insert_link_by(&pool, "共有リンク", "public", Some(bob_id), None).await;
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_empty(app, "GET", "/api/v1/admin/users", &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let json: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めない");
    let bob = json
        .as_array()
        .expect("配列でない")
        .iter()
        .find(|user| user["username"] == "bob")
        .unwrap_or_else(|| panic!("bobが見つからない: {json}"));
    assert_eq!(bob["privateContentCount"], 2, "{json}");
}
