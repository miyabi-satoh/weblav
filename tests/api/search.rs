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

/// コンテンツはタイトルと説明で、一覧の表示を使うフォルダーのファイルは表示タイトルで当たる。
/// 絞り込みで外れるファイルは出さない。
#[sqlx::test]
async fn search_finds_contents_and_visible_archive_files(pool: SqlitePool) {
    let dir = temp_test_dir("search");
    std::fs::write(dir.join("リスニング第1回.mp3"), b"").expect("ファイルを作れなかった");
    std::fs::write(dir.join("リスニング第2回.mp3"), b"").expect("ファイルを作れなかった");
    let (app, _cookie, archive_id) = setup_archive(&pool, &dir, None).await;
    sqlx::query!(
        "UPDATE contents SET exclude_patterns = 'リスニング第2回.*' WHERE id = ?",
        archive_id
    )
    .execute(&pool)
    .await
    .expect("パターンを書けなかった");
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

    // 一覧の表示の索引で当たった行も、フォルダーの中の当たりと同じ並びに入る。
    let files = found["files"].as_array().expect("files は配列");
    assert_eq!(files.len(), 1, "{found}");
    assert_eq!(files[0]["contentId"], archive_id, "{found}");
    assert_eq!(files[0]["folderTitle"], "英検 過去問", "{found}");
    assert_eq!(files[0]["ancestors"], serde_json::json!([]), "{found}");
    assert_eq!(files[0]["path"], "リスニング第1回.mp3", "{found}");
    assert_eq!(
        files[0]["listed"]["title"], "リスニング第1回.mp3",
        "{found}"
    );
    // 行はフォルダーの口で開く。
    let (status, body) = send_anon(
        app.clone(),
        "GET",
        &format!(
            "/api/v1/contents/{archive_id}/download?path={}",
            utf8_percent_encode("リスニング第1回.mp3", NON_ALPHANUMERIC)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let contents = found["contents"].as_array().expect("contents は配列");
    assert_eq!(contents.len(), 1, "{found}");
    assert_eq!(contents[0]["content"]["id"], link_id, "{found}");
    assert_eq!(contents[0]["ancestors"][0]["title"], "教材", "{found}");
    assert_eq!(contents[0]["matchedInDescription"], true, "{found}");
    assert_eq!(found["contentsTruncated"], false, "{found}");
    assert_eq!(found["filesTruncated"], false, "{found}");
}

/// 語が空なら探さない。
#[sqlx::test]
async fn search_with_a_blank_query_returns_nothing(pool: SqlitePool) {
    insert_group_under(&pool, "教材", "public", None).await;
    let app = test_app(pool).await;

    let found = search(app, " ").await;

    assert_eq!(found["contents"], serde_json::json!([]), "{found}");
    assert_eq!(found["files"], serde_json::json!([]), "{found}");
}

/// フォルダーの中はファイルとディレクトリの名前で、リンクの一覧のファイルの中は題と `note` で当たる。
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
    let group_id = insert_group_under(&pool, "高1", "public", None).await;
    let folder_id = insert_folder_by(
        &pool,
        "教材",
        dir.display().to_string(),
        "public",
        None,
        Some(group_id),
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
    assert_eq!(files[0]["ancestors"][0]["title"], "高1", "{found}");
    assert_eq!(files[0]["entry"]["isDir"], true, "{found}");
    let links = found["links"].as_array().expect("links は配列");
    assert_eq!(links.len(), 1, "{found}");
    assert_eq!(links[0]["contentId"], folder_id, "{found}");
    assert_eq!(links[0]["path"], "今朝.links.toml", "{found}");
    assert_eq!(links[0]["fileTitle"], "今朝の記事", "{found}");
    assert_eq!(links[0]["containerTitle"], "教材", "{found}");
    assert_eq!(links[0]["ancestors"][0]["id"], group_id, "{found}");
    assert_eq!(links[0]["url"], "https://example.com/", "{found}");
    assert_eq!(links[0]["matchedInNote"], true, "{found}");
    assert_eq!(found["filesTruncated"], false, "{found}");
    assert_eq!(found["linksTruncated"], false, "{found}");
    assert_eq!(found["foldersIncomplete"], false, "{found}");
}

fn search_uri(query: &str, scope: &str) -> String {
    format!(
        "/api/v1/search?q={}&{scope}",
        utf8_percent_encode(query, NON_ALPHANUMERIC)
    )
}

/// グループを範囲にすると、その子孫だけを探す。`all` で全体に戻しても、範囲の名前は返す。
#[sqlx::test]
async fn search_within_a_group_finds_only_its_descendants(pool: SqlitePool) {
    let group_id = insert_group_under(&pool, "教材", "public", None).await;
    let inner_id = insert_group_under(&pool, "2024", "public", Some(group_id)).await;
    let inside = insert_link_by(&pool, "リスニング中", "public", None, Some(inner_id)).await;
    let outside = insert_link_by(&pool, "リスニング外", "public", None, None).await;
    let app = test_app(pool).await;

    let (status, body) = send_anon(
        app.clone(),
        "GET",
        &search_uri("リスニング", &format!("within={group_id}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let found = parse_json(&body);
    let ids: Vec<&serde_json::Value> = found["contents"]
        .as_array()
        .expect("contents は配列")
        .iter()
        .map(|hit| &hit["content"]["id"])
        .collect();
    assert_eq!(ids, vec![&serde_json::json!(inside)], "{found}");
    assert_eq!(found["scopeTitle"], "教材", "{found}");

    let (status, body) = send_anon(
        app,
        "GET",
        &search_uri("リスニング", &format!("within={group_id}&all=true")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let found = parse_json(&body);
    assert_eq!(
        found["contents"].as_array().expect("contents は配列").len(),
        2,
        "{found}"
    );
    assert_eq!(found["contents"][1]["content"]["id"], outside, "{found}");
    assert_eq!(found["scopeTitle"], "教材", "{found}");
}

/// フォルダーの中の階層を範囲にすると、その下だけを辿り、パスは登録パスからの相対で返す。
/// コンテンツの区画は探さない。
#[sqlx::test]
async fn search_within_a_folder_level_walks_only_below_it(pool: SqlitePool) {
    let dir = temp_test_dir("search-scope-folder");
    std::fs::create_dir_all(dir.join("2024")).expect("ディレクトリを作れなかった");
    std::fs::create_dir_all(dir.join("2023")).expect("ディレクトリを作れなかった");
    std::fs::write(dir.join("2024/リスニング.pdf"), b"").expect("ファイルを作れなかった");
    std::fs::write(dir.join("2023/リスニング.pdf"), b"").expect("ファイルを作れなかった");
    let folder_id = insert_folder_by(
        &pool,
        "リスニング教材",
        dir.display().to_string(),
        "public",
        None,
        None,
    )
    .await;
    let app = test_app(pool).await;

    let (status, body) = send_anon(
        app,
        "GET",
        &search_uri("リスニング", &format!("within={folder_id}&path=2024")),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let found = parse_json(&body);
    assert_eq!(found["contents"], serde_json::json!([]), "{found}");
    let paths: Vec<&str> = found["files"]
        .as_array()
        .expect("files は配列")
        .iter()
        .map(|hit| hit["path"].as_str().expect("path は文字列"))
        .collect();
    assert_eq!(paths, vec!["2024/リスニング.pdf"], "{found}");
    assert_eq!(found["scopeTitle"], "リスニング教材", "{found}");
}

/// 範囲の場所を開けなければ、その画面と同じく 401/404 にする。
/// 範囲にできる種類と階層の形は `src/api/contents.rs` の単体テストで場合ごとに見る。
#[sqlx::test]
async fn search_within_rejects_places_that_cannot_be_opened(pool: SqlitePool) {
    let members = insert_group_under(&pool, "ログインした人", "authenticated", None).await;
    let link = insert_link_by(&pool, "リンク", "public", None, None).await;
    let app = test_app(pool).await;

    let (status, body) = send_anon(
        app.clone(),
        "GET",
        &search_uri("a", &format!("within={members}")),
    )
    .await;
    assert_error(status, &body, StatusCode::UNAUTHORIZED, "unauthorized");
    let (status, body) = send_anon(app, "GET", &search_uri("a", &format!("within={link}"))).await;
    assert_error(status, &body, StatusCode::NOT_FOUND, "not_found");
}

/// フォルダーの中のファイルは、途中のフォルダー名と飛ばし打ちでも当たり、よく当たるものから並ぶ。
/// 照らし方と並べ方の場合分けは `src/api/search.rs` の単体テストで見る。
#[sqlx::test]
async fn search_matches_places_and_skipped_letters_and_ranks_better_matches_first(
    pool: SqlitePool,
) {
    let dir = temp_test_dir("search-fuzzy");
    std::fs::create_dir_all(dir.join("mugenp")).expect("ディレクトリを作れなかった");
    std::fs::write(dir.join("mugenp/review-units.mp4"), b"").expect("ファイルを作れなかった");
    std::fs::write(dir.join("preview.mp4"), b"").expect("ファイルを作れなかった");
    insert_folder_by(
        &pool,
        "knowledge",
        dir.display().to_string(),
        "public",
        None,
        None,
    )
    .await;
    let app = test_app(pool).await;
    let paths = |found: &serde_json::Value| -> Vec<String> {
        found["files"]
            .as_array()
            .expect("files は配列")
            .iter()
            .map(|hit| hit["path"].as_str().expect("path は文字列").to_string())
            .collect()
    };

    let found = search(app.clone(), "mugenp review mp4").await;
    assert_eq!(paths(&found), vec!["mugenp/review-units.mp4"], "{found}");

    // 場所の語だけなら、場所そのものだけが当たる。
    let found = search(app.clone(), "mugenp").await;
    assert_eq!(paths(&found), vec!["mugenp"], "{found}");

    // 飛ばし打ちでも当たり、語の頭から当たるものが中に含むものより先に並ぶ。
    let found = search(app.clone(), "rvwunit").await;
    assert_eq!(paths(&found), vec!["mugenp/review-units.mp4"], "{found}");
    let found = search(app, "review").await;
    assert_eq!(
        paths(&found),
        vec!["mugenp/review-units.mp4", "preview.mp4"],
        "{found}"
    );
}

/// ログインした人に見える共有の中を、`shares` の区画で返す。`files` に出したファイルは重ねて出さない。
#[sqlx::test]
async fn search_finds_entries_in_readable_shares_for_logged_in_users(pool: SqlitePool) {
    let share = temp_root_dir("search-share");
    std::fs::create_dir_all(share.join("教材/公開")).expect("ディレクトリを作れなかった");
    std::fs::write(share.join("教材/リスニング.pdf"), b"").expect("ファイルを作れなかった");
    std::fs::write(share.join("教材/公開/リスニング公開.pdf"), b"")
        .expect("ファイルを作れなかった");
    let (share_id, share_name) = register_search_share(&pool, &share).await;
    let folder_id = insert_folder_by(
        &pool,
        "公開",
        share.join("教材/公開").display().to_string(),
        "public",
        None,
        None,
    )
    .await;
    insert_user(&pool, "taro", "password").await;
    let app = test_app(pool.clone()).await;
    only_share_readable(&pool, share_id).await;
    let cookie = login(app.clone(), "taro", "password").await;

    let (status, body) =
        send_empty(app.clone(), "GET", &search_uri("リスニング", ""), &cookie).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let found = parse_json(&body);
    assert_eq!(found["files"][0]["contentId"], folder_id, "{found}");
    let shares = found["shares"].as_array().expect("shares は配列");
    assert_eq!(shares.len(), 1, "{found}");
    assert_eq!(shares[0]["shareId"], share_id, "{found}");
    assert_eq!(shares[0]["shareName"], share_name.as_str(), "{found}");
    assert_eq!(shares[0]["path"], "教材/リスニング.pdf", "{found}");
    assert_eq!(found["sharesTruncated"], false, "{found}");
    // ログインしていなければ、共有の中は探さない。
    let found = search(app, "リスニング").await;
    assert_eq!(found["shares"], serde_json::json!([]), "{found}");
}

/// 共有の中の階層を範囲にすると、その下だけを辿り、パスは共有からの相対で返す。コンテンツの区画は探さない。
/// 見られない共有は 404、未ログインは 401。
#[sqlx::test]
async fn search_within_a_share_walks_only_below_it(pool: SqlitePool) {
    let share = temp_root_dir("search-share-scope");
    std::fs::create_dir_all(share.join("2024")).expect("ディレクトリを作れなかった");
    std::fs::create_dir_all(share.join("2023")).expect("ディレクトリを作れなかった");
    std::fs::write(share.join("2024/リスニング.pdf"), b"").expect("ファイルを作れなかった");
    std::fs::write(share.join("2023/リスニング.pdf"), b"").expect("ファイルを作れなかった");
    let (share_id, share_name) = register_search_share(&pool, &share).await;
    insert_link_by(&pool, "リスニングのページ", "public", None, None).await;
    insert_user(&pool, "taro", "password").await;
    let app = test_app(pool.clone()).await;
    only_share_readable(&pool, share_id).await;
    let cookie = login(app.clone(), "taro", "password").await;

    let (status, body) = send_empty(
        app.clone(),
        "GET",
        &search_uri("リスニング", &format!("share={share_id}&path=2024")),
        &cookie,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let found = parse_json(&body);
    assert_eq!(found["contents"], serde_json::json!([]), "{found}");
    let paths: Vec<&str> = found["shares"]
        .as_array()
        .expect("shares は配列")
        .iter()
        .map(|hit| hit["path"].as_str().expect("path は文字列"))
        .collect();
    assert_eq!(paths, vec!["2024/リスニング.pdf"], "{found}");
    assert_eq!(found["scopeTitle"], share_name.as_str(), "{found}");

    let hidden: i64 = sqlx::query_scalar("SELECT id FROM roots WHERE id != ?")
        .bind(share_id)
        .fetch_one(&pool)
        .await
        .expect("土台の共有を引けなかった");
    let (status, body) = send_empty(
        app.clone(),
        "GET",
        &search_uri("a", &format!("share={hidden}")),
        &cookie,
    )
    .await;
    assert_error(status, &body, StatusCode::NOT_FOUND, "not_found");
    let (status, body) =
        send_anon(app, "GET", &search_uri("a", &format!("share={share_id}"))).await;
    assert_error(status, &body, StatusCode::UNAUTHORIZED, "unauthorized");
}

/// 共有を登録し、id と名前を返す。
async fn register_search_share(pool: &SqlitePool, dir: &std::path::Path) -> (i64, String) {
    let canonical = std::fs::canonicalize(dir).expect("canonicalize できなかった");
    register_root(pool, &canonical).await;
    let name = canonical_path_as_api_returns_it(&canonical);
    let id = sqlx::query_scalar("SELECT id FROM roots WHERE path = ?")
        .bind(&name)
        .fetch_one(pool)
        .await
        .expect("共有の行を引けなかった");
    (id, name)
}

/// 一般ユーザーが見られる共有を `share_id` だけにする。土台の共有にはほかのテストのフォルダーも入っていて、
/// 当たりが揃わないため。
async fn only_share_readable(pool: &SqlitePool, share_id: i64) {
    sqlx::query("UPDATE roots SET editor_access = 'none' WHERE id != ?")
        .bind(share_id)
        .execute(pool)
        .await
        .expect("共有の権限を書けなかった");
}
