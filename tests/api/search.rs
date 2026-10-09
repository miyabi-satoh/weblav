//! 閲覧の場の検索 (→ docs/search.md)。
//! 一致の決まりと見える範囲は `src/api/search.rs`・`src/api/contents.rs` の単体テストで場合ごとに見る。

use super::*;

async fn search(app: Router, query: &str) -> serde_json::Value {
    let (status, body) = send_anon(
        app,
        "GET",
        &format!(
            "/api/v1/search?q={}",
            utf8_percent_encode(query, NON_ALPHANUMERIC)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    parse_json(&body)
}

/// コンテンツはタイトルと説明で、アーカイブのファイルは表示タイトルで当たる。
/// アーカイブの非公開のアイテムは出さない。
#[sqlx::test]
async fn search_finds_contents_and_published_archive_files(pool: SqlitePool) {
    let dir = temp_test_dir("search");
    std::fs::write(dir.join("リスニング第1回.mp3"), b"").expect("ファイルを作れなかった");
    std::fs::write(dir.join("リスニング第2回.mp3"), b"").expect("ファイルを作れなかった");
    let (app, _cookie, archive_id) = setup_archive(&pool, &dir, None).await;
    sqlx::query!(
        "UPDATE archive_items SET published = 1 WHERE archive_id = ? AND rel_path = 'リスニング第1回.mp3'",
        archive_id
    )
    .execute(&pool)
    .await
    .expect("公開フラグを立てられなかった");
    let group_id = insert_group_under(&pool, "教材", "public", None).await;
    let link_id = insert_link_by(&pool, "英語のページ", "public", None, Some(group_id)).await;
    sqlx::query!(
        "UPDATE contents SET description = 'リスニングの練習' WHERE id = ?",
        link_id
    )
    .execute(&pool)
    .await
    .expect("説明を書けなかった");

    let found = search(app.clone(), "リスニング").await;

    let items = found["items"].as_array().expect("items は配列");
    assert_eq!(items.len(), 1, "{found}");
    assert_eq!(items[0]["archiveId"], archive_id, "{found}");
    assert_eq!(items[0]["archiveTitle"], "英検 過去問", "{found}");
    assert_eq!(items[0]["item"]["title"], "リスニング第1回.mp3", "{found}");
    let contents = found["contents"].as_array().expect("contents は配列");
    assert_eq!(contents.len(), 1, "{found}");
    assert_eq!(contents[0]["content"]["id"], link_id, "{found}");
    assert_eq!(contents[0]["parent"]["title"], "教材", "{found}");
    assert_eq!(contents[0]["matchedInDescription"], true, "{found}");
    assert_eq!(found["contentsTruncated"], false, "{found}");
    assert_eq!(found["itemsTruncated"], false, "{found}");
}

/// 語が空なら探さない。
#[sqlx::test]
async fn search_with_a_blank_query_returns_nothing(pool: SqlitePool) {
    insert_group_under(&pool, "教材", "public", None).await;
    let app = test_app(pool).await;

    let found = search(app, " ").await;

    assert_eq!(found["contents"], serde_json::json!([]), "{found}");
    assert_eq!(found["items"], serde_json::json!([]), "{found}");
}

/// フォルダの中はファイルとディレクトリの名前で、リンクの一覧のファイルの中は題と `note` で当たる。
/// 辿り方の除外 (ドット始まりなど) は `src/api/fs.rs` の単体テストで見る。
#[sqlx::test]
async fn search_finds_entries_in_folders_and_links_in_links_files(pool: SqlitePool) {
    let dir = temp_test_dir("search-folder");
    std::fs::create_dir_all(dir.join("2024/リスニング")).expect("ディレクトリを作れなかった");
    std::fs::write(dir.join("2024/リスニング/第3回.mp3"), b"").expect("ファイルを作れなかった");
    std::fs::write(dir.join("2024/リスニング問題.pdf"), b"").expect("ファイルを作れなかった");
    std::fs::write(
        dir.join("今朝.links.toml"),
        "title = \"今朝の記事\"\n\n[[links]]\nurl = \"https://example.com\"\nnote = \"リスニングの練習\"\n",
    )
    .expect("ファイルを作れなかった");
    let folder_id = insert_folder_by(
        &pool,
        "教材",
        dir.display().to_string(),
        "public",
        None,
        None,
    )
    .await;
    insert_fresh_preview(&pool, "https://example.com/", None).await;
    let app = test_app(pool).await;

    let found = search(app, "リスニング").await;

    let files = found["files"].as_array().expect("files は配列");
    let paths: Vec<&str> = files
        .iter()
        .map(|hit| hit["path"].as_str().expect("path は文字列"))
        .collect();
    assert_eq!(
        paths,
        vec!["2024/リスニング", "2024/リスニング問題.pdf"],
        "{found}"
    );
    assert_eq!(files[0]["contentId"], folder_id, "{found}");
    assert_eq!(files[0]["folderTitle"], "教材", "{found}");
    assert_eq!(files[0]["entry"]["isDir"], true, "{found}");
    let links = found["links"].as_array().expect("links は配列");
    assert_eq!(links.len(), 1, "{found}");
    assert_eq!(links[0]["contentId"], folder_id, "{found}");
    assert_eq!(links[0]["path"], "今朝.links.toml", "{found}");
    assert_eq!(links[0]["fileTitle"], "今朝の記事", "{found}");
    assert_eq!(links[0]["containerTitle"], "教材", "{found}");
    assert_eq!(links[0]["url"], "https://example.com/", "{found}");
    assert_eq!(links[0]["matchedInNote"], true, "{found}");
    assert_eq!(found["filesTruncated"], false, "{found}");
    assert_eq!(found["linksTruncated"], false, "{found}");
}
