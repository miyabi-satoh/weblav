//! リンクの一覧のファイル (→ docs/ui.md「リンクの一覧のファイル」)。読み方の決まりは `src/api/links_file.rs` の単体テストで見る。
//! ここは外へつながないよう、期限の切れていないカードの情報を先に入れておく。

use super::*;

const LINKS_TOML: &str = r#"
title = "今朝の記事"

[[links]]
url = "https://example.com"
note = "読んでおく"
"#;

/// 一時ディレクトリは、返した値を持っている間だけ残る。
async fn public_folder_with(
    pool: &SqlitePool,
    name: &str,
    file: &str,
    contents: &[u8],
) -> (test_support::TempDir, i64) {
    let dir = temp_test_dir(name);
    std::fs::write(dir.join(file), contents).expect("書けるはず");
    let id = insert_folder_by(
        pool,
        "教材",
        dir.display().to_string(),
        "public",
        None,
        None,
    )
    .await;
    (dir, id)
}

#[sqlx::test]
async fn links_file_in_a_folder_lists_its_links_with_the_remembered_preview(pool: SqlitePool) {
    let (_dir, id) = public_folder_with(
        &pool,
        "links-file",
        "今朝.links.toml",
        LINKS_TOML.as_bytes(),
    )
    .await;
    insert_fresh_preview(&pool, "https://example.com/", None).await;
    let app = test_app(pool).await;

    let (status, body) = send_anon(
        app,
        "GET",
        &format!("/api/v1/contents/{id}/links?path=%E4%BB%8A%E6%9C%9D.links.toml"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let json = parse_json(&body);
    assert_eq!(json["title"], "今朝の記事", "{json}");
    assert_eq!(json["name"], "今朝.links.toml", "{json}");
    assert_eq!(json["containerTitle"], "教材", "{json}");
    assert_eq!(json["links"][0]["url"], "https://example.com/", "{json}");
    assert_eq!(json["links"][0]["note"], "読んでおく", "{json}");
    assert_eq!(
        json["links"][0]["preview"]["siteName"], "Example Site",
        "{json}"
    );
}

/// 読めない一覧は、画面が読めなかった旨を出せるよう、リンクを無しにしてファイル名を題にする。
#[sqlx::test]
async fn unreadable_links_file_answers_without_links(pool: SqlitePool) {
    let (_dir, id) =
        public_folder_with(&pool, "links-file-broken", "a.links.toml", b"links = [\n").await;
    let app = test_app(pool).await;

    let (status, body) = send_anon(
        app,
        "GET",
        &format!("/api/v1/contents/{id}/links?path=a.links.toml"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let json = parse_json(&body);
    assert_eq!(json["title"], "a.links.toml", "{json}");
    assert!(json["links"].is_null(), "{json}");
}

#[sqlx::test]
async fn other_files_are_not_read_as_links_files(pool: SqlitePool) {
    let (_dir, id) =
        public_folder_with(&pool, "links-file-other", "a.toml", LINKS_TOML.as_bytes()).await;
    let app = test_app(pool).await;

    let (status, body) = send_anon(
        app,
        "GET",
        &format!("/api/v1/contents/{id}/links?path=a.toml"),
    )
    .await;
    assert_error(status, &body, StatusCode::NOT_FOUND, "not_found");
}

#[sqlx::test]
async fn refresh_answers_with_the_previews_of_the_links_in_a_links_file(pool: SqlitePool) {
    let (_dir, id) = public_folder_with(
        &pool,
        "links-file-refresh",
        "a.links.toml",
        LINKS_TOML.as_bytes(),
    )
    .await;
    insert_fresh_preview(&pool, "https://example.com/", None).await;
    let app = test_app(pool).await;

    let (status, body) = send_anon_json(
        app,
        "POST",
        "/api/v1/link-previews/refresh",
        &format!(r#"{{"contentIds":[],"linksFile":{{"contentId":{id},"path":"a.links.toml"}}}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let json = parse_json(&body);
    assert_eq!(json[0]["url"], "https://example.com/", "{json}");
    assert_eq!(json[0]["preview"]["title"], "Example", "{json}");
}

#[sqlx::test]
async fn links_file_in_an_archive_is_opened_by_its_item(pool: SqlitePool) {
    let dir = temp_test_dir("links-file-archive");
    std::fs::write(dir.join("a.links.toml"), LINKS_TOML).expect("書けるはず");
    let (app, cookie, id) = setup_archive(&pool, &dir, None).await;
    publish_all_items(&pool, id).await;
    let item_id: i64 = sqlx::query_scalar("SELECT id FROM archive_items WHERE archive_id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("アイテムがあるはず");

    let (status, body) = send_empty(
        app,
        "GET",
        &format!("/api/v1/contents/{id}/links?item={item_id}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let json = parse_json(&body);
    assert_eq!(json["name"], "a.links.toml", "{json}");
    assert!(json["containerTitle"].is_string(), "{json}");
    assert_eq!(json["links"][0]["url"], "https://example.com/", "{json}");
}

/// 取り込んだファイルは、一覧の行と同じく登録したタイトルをパンくずに出す。
#[sqlx::test]
async fn links_file_content_is_named_by_its_registered_title(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    let response = send_raw(
        app.clone(),
        multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "今朝のリンク"), ("visibility", "public")],
            Some(("file", "a.links.toml", LINKS_TOML.as_bytes())),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let id = created_id(response).await;

    let (status, body) = send_anon(app, "GET", &format!("/api/v1/contents/{id}/links")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let json = parse_json(&body);
    assert_eq!(json["name"], "今朝のリンク", "{json}");
    assert_eq!(json["title"], "今朝の記事", "{json}");
    assert!(json["containerTitle"].is_null(), "{json}");
}
