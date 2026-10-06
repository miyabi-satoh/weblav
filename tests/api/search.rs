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
