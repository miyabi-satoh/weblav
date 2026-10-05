//! コンテンツの一覧・作成・更新・削除。

use super::*;

/// 匿名でも一覧は200で返り、`public` のものだけが並ぶことを確認する
/// (→ docs/access.md「匿名閲覧の受け口」。一覧は401にせずWHERE句で絞る)。
#[sqlx::test]
async fn contents_list_without_cookie_returns_only_public(pool: SqlitePool) {
    insert_link_by(&pool, "公開Wiki", "public", None, None).await;
    insert_link_by(&pool, "社内Wiki", "authenticated", None, None).await;
    let app = test_app(pool).await;

    let (status, body) = send_anon(app, "GET", "/api/v1/contents").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("公開Wiki"), "{body}");
    assert!(!body.contains("社内Wiki"), "{body}");
}

/// ログイン済みには `public` と `authenticated` の両方が見えることを確認する
/// (visibilityの絞り込みを`= 'authenticated'`のままにすると公開分が消える、という退行の防止)。
#[sqlx::test]
async fn contents_list_for_logged_in_user_includes_public_and_authenticated(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    insert_link_by(&pool, "公開Wiki", "public", None, None).await;
    insert_link_by(&pool, "社内Wiki", "authenticated", None, None).await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let (status, body) = send_empty(app, "GET", "/api/v1/contents", &cookie).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("公開Wiki"), "{body}");
    assert!(body.contains("社内Wiki"), "{body}");
}

/// 一覧に並ぶタイトルを、返ってきた順のまま取り出す。
async fn list_titles(app: Router, uri: &str) -> Vec<String> {
    let (status, body) = send_anon(app, "GET", uri).await;
    assert_eq!(status, StatusCode::OK);
    let entries: serde_json::Value =
        serde_json::from_str(&body).expect("一覧のレスポンスを読めなかった");
    // グループの一覧 (`entries` を持つ) とトップの一覧 (配列そのもの) の両方を受ける。
    let array = entries
        .get("entries")
        .unwrap_or(&entries)
        .as_array()
        .expect("一覧が配列ではなかった")
        .clone();
    array
        .iter()
        .map(|entry| entry["title"].as_str().expect("titleが無い").to_string())
        .collect()
}

/// 既定の並びはタイトル順で、数字は数値として比べる (→ docs/ui.md「ホーム・グループ・フォルダ・アーカイブの並び順」)。
/// 追加した順 (id順) とは違う並びになる材料を入れてある。比べ方の場合分けは `title_cmp` の単体テストで見る。
#[sqlx::test]
async fn contents_list_sorts_by_title_with_numbers_as_numbers(pool: SqlitePool) {
    for title in ["第10回", "apple", "第2回", "Banana"] {
        insert_link_by(&pool, title, "public", None, None).await;
    }
    let app = test_app(pool).await;

    assert_eq!(
        list_titles(app, "/api/v1/contents").await,
        vec!["apple", "Banana", "第2回", "第10回"]
    );
}

/// `sort=new` は追加した日時の降順。知らない値・空を既定に戻すことは `SortOrder::from_query` の単体テストで見る。
#[sqlx::test]
async fn contents_list_sorts_by_created_at_when_new_is_requested(pool: SqlitePool) {
    for (title, created_at) in [
        ("古い", "2026-09-01T00:00:00.000Z"),
        ("新しい", "2026-09-10T00:00:00.000Z"),
        ("中くらい", "2026-09-05T00:00:00.000Z"),
    ] {
        sqlx::query(
            "INSERT INTO contents (type, title, url, visibility, created_at) \
             VALUES ('link', ?, 'https://example.com', 'public', ?)",
        )
        .bind(title)
        .bind(created_at)
        .execute(&pool)
        .await
        .expect("リンクを入れられなかった");
    }
    let app = test_app(pool).await;

    assert_eq!(
        list_titles(app, "/api/v1/contents?sort=new").await,
        vec!["新しい", "中くらい", "古い"]
    );
}

/// グループの中の一覧も同じ `sort` を受ける。
#[sqlx::test]
async fn group_browse_accepts_the_same_sort_query(pool: SqlitePool) {
    let group_id = insert_group_under(&pool, "グループ", "public", None).await;
    for (title, created_at) in [
        ("第10回", "2026-09-10T00:00:00.000Z"),
        ("第2回", "2026-09-01T00:00:00.000Z"),
    ] {
        sqlx::query(
            "INSERT INTO contents (type, parent_id, title, url, visibility, created_at) \
             VALUES ('link', ?, ?, 'https://example.com', 'public', ?)",
        )
        .bind(group_id)
        .bind(title)
        .bind(created_at)
        .execute(&pool)
        .await
        .expect("リンクを入れられなかった");
    }
    let app = test_app(pool).await;

    let uri = format!("/api/v1/contents/{group_id}/group");
    assert_eq!(
        list_titles(app.clone(), &uri).await,
        vec!["第2回", "第10回"]
    );
    assert_eq!(
        list_titles(app, &format!("{uri}?sort=new")).await,
        vec!["第10回", "第2回"]
    );
}

/// 一般ユーザーも link を作れて、公開範囲も選べる (→ docs/access.md「ロールと操作」)。
#[sqlx::test]
async fn contents_create_link_by_regular_user_keeps_requested_visibility(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/contents",
        &cookie,
        r#"{"type":"link","title":"社内Wiki","url":"https://example.com","visibility":"authenticated"}"#,
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert!(body.contains(r#""visibility":"authenticated""#), "{body}");
}

/// 編集者も folder を作れる。選べるのは「公開できるフォルダ」の中だけなので、
/// 誰が選んでも範囲は同じ (→ docs/access.md「ロールと操作」・docs/folders.md「公開できるフォルダ」)。
#[sqlx::test]
async fn contents_create_folder_by_regular_user_succeeds(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let dir = temp_test_dir("create-folder-by-user");
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/contents",
        &cookie,
        &format!(
            r#"{{"type":"folder","title":"教材","path":{}}}"#,
            json_string(&dir.display().to_string())
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[sqlx::test]
async fn contents_create_with_empty_title_is_validation_error(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/contents",
        &cookie,
        r#"{"type":"group","title":"  "}"#,
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
async fn contents_create_with_non_http_url_is_validation_error(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/contents",
        &cookie,
        r#"{"type":"link","title":"XSS","url":"javascript:alert(1)"}"#,
    )
    .await;

    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
}

/// プロトコルを省略した URL は補って保存する。補い方の場合分けは `resolve_url` の単体テストで見る。
#[sqlx::test]
async fn contents_create_link_without_scheme_to_a_lan_host_gets_http_prefixed(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/contents",
        &cookie,
        r#"{"type":"link","title":"Wiki","url":"192.168.0.10/wiki"}"#,
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(
        body.contains(r#""url":"http://192.168.0.10/wiki""#),
        "{body}"
    );
}

/// プライベートアドレスへは接続せず、タイトル・説明とも `null` を返す
/// (`link_title::PublicOnlyResolver` 経由。実ネットワークへは繋がない)。
#[sqlx::test]
async fn link_metadata_for_a_private_address_returns_empty_metadata(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/contents/link-metadata",
        &cookie,
        r#"{"url":"http://127.0.0.1:9/"}"#,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(r#""title":null"#) && body.contains(r#""description":null"#),
        "{body}"
    );
}

/// admin による作成→一覧への反映→更新→削除→削除後は404、という一連の流れを確認する。
#[sqlx::test]
async fn contents_crud_round_trip_as_admin(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (create_status, create_body) = send_json(app.clone(), "POST", "/api/v1/contents", &cookie, r#"{"type":"link","title":"社内Wiki","url":"https://example.com","description":"社内向け"}"#).await;
    assert_eq!(create_status, StatusCode::CREATED);
    assert!(create_body.contains("社内Wiki"), "{create_body}");
    let id = extract_id(&create_body);

    let (list_status, list_body) =
        send_empty(app.clone(), "GET", "/api/v1/contents", &cookie).await;
    assert_eq!(list_status, StatusCode::OK);
    assert!(list_body.contains("社内Wiki"), "{list_body}");

    let (update_status, update_body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}"),
        &cookie,
        r#"{"title":"社内Wiki(改)","url":"https://example.com/updated"}"#,
    )
    .await;
    assert_eq!(update_status, StatusCode::OK);
    assert!(update_body.contains("社内Wiki(改)"), "{update_body}");

    let (delete_status, _) = send_empty(
        app.clone(),
        "DELETE",
        &format!("/api/v1/contents/{id}"),
        &cookie,
    )
    .await;
    assert_eq!(delete_status, StatusCode::NO_CONTENT);

    // 存在しないidの再削除は404。
    let (redelete_status, _) =
        send_empty(app, "DELETE", &format!("/api/v1/contents/{id}"), &cookie).await;
    assert_eq!(redelete_status, StatusCode::NOT_FOUND);
}

/// 一般ユーザーも一覧(GET)は見えることを確認する。
#[sqlx::test]
async fn contents_list_succeeds_for_regular_user(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    insert_link_by(&pool, "社内Wiki", "authenticated", None, None).await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let (status, body) = send_empty(app, "GET", "/api/v1/contents", &cookie).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("社内Wiki"), "{body}");
}

/// 一覧APIはルート直下(parent_id IS NULL)のみを返すことを確認する。ルート直下のgroupは
/// 正当なtypeとして含まれる一方、そのgroup配下(非ルート)に置いたlinkは含まれない。
#[sqlx::test]
async fn contents_list_returns_root_group_and_excludes_grouped_link(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let group_id = insert_group_under(&pool, "グループ", "authenticated", None).await;
    insert_link_by(
        &pool,
        "グループ内Wiki",
        "authenticated",
        None,
        Some(group_id),
    )
    .await;
    insert_link_by(&pool, "社内Wiki", "authenticated", None, None).await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let (status, body) = send_empty(app, "GET", "/api/v1/contents", &cookie).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"type\":\"group\""), "{body}");
    assert!(body.contains("社内Wiki"), "{body}");
    assert!(!body.contains("グループ内Wiki"), "{body}");
}

/// 一般ユーザーは、他人のコンテンツも作成者のいないコンテンツも書き換えられない。
/// 一覧には出るので、存在を隠す404ではなく403にする (→ docs/access.md「ロールと操作」)。
#[sqlx::test]
async fn contents_update_and_delete_by_regular_user_are_forbidden_for_others(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    insert_user(&pool, "bob", "correct-password").await;
    let bob_id = db_user_id(&pool, "bob").await;
    let ownerless = insert_link_by(&pool, "社内Wiki", "authenticated", None, None).await;
    let bobs = insert_link_by(&pool, "ボブのリンク", "public", Some(bob_id), None).await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    for id in [ownerless, bobs] {
        let (status, body) = send_json(
            app.clone(),
            "PUT",
            &format!("/api/v1/contents/{id}"),
            &cookie,
            r#"{"title":"改題","url":"https://example.com"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

        let (status, body) = send_empty(
            app.clone(),
            "DELETE",
            &format!("/api/v1/contents/{id}"),
            &cookie,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    }
}

/// 一般ユーザーは、自分が作った link も folder も削除できる (→ docs/access.md「ロールと操作」)。
#[sqlx::test]
async fn contents_delete_by_regular_user_allows_link_and_folder(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let alice_id = db_user_id(&pool, "alice").await;
    let dir = temp_test_dir("delete-folder-by-user");
    let link_id = insert_link_by(&pool, "社内Wiki", "authenticated", Some(alice_id), None).await;
    let folder_id = insert_folder_by(
        &pool,
        "教材",
        dir.display().to_string(),
        "authenticated",
        Some(alice_id),
        None,
    )
    .await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let (status, _) = send_empty(
        app.clone(),
        "DELETE",
        &format!("/api/v1/contents/{link_id}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = send_empty(
        app,
        "DELETE",
        &format!("/api/v1/contents/{folder_id}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}
