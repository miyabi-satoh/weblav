//! アイテムの一覧・公開切り替え・配信 (→ docs/archive.md「アイテムの公開」・「アイテムの配信」・「エンドポイント一覧」)

use super::*;

/// ディレクトリ・フォルダの一覧の応答から、エントリの名前を並び順のまま取り出す。
fn entry_names(body: &str) -> Vec<String> {
    let json: serde_json::Value = serde_json::from_str(body).expect("一覧を読めなかった");
    json["entries"]
        .as_array()
        .expect("entries が無い")
        .iter()
        .filter_map(|entry| entry["name"].as_str().map(str::to_string))
        .collect()
}

/// 軸を1本(`filenameWord`)持つアーカイブをセットアップし、
/// `(app, cookie, archive_id, dir)` を返す。
/// `dir` は Drop で消えるので、使わなくてもテストの終わりまで持つ (`_dir` で受ける。`_` だとすぐ消える)。
async fn setup_archive_with_filename_word_axis(
    pool: SqlitePool,
) -> (Router, String, i64, test_support::TempDir) {
    insert_admin(&pool, "admin-alice", "correct-password").await;
    let dir = temp_test_dir("archive-items");
    std::fs::write(dir.join("2024_kokugo.pdf"), b"").expect("ファイルを作れなかった");
    std::fs::write(dir.join("2024_eigo.pdf"), b"").expect("ファイルを作れなかった");
    std::fs::write(dir.join("2024_unknown.pdf"), b"").expect("ファイルを作れなかった");

    let app = test_app(pool.clone()).await;
    let cookie = login(app.clone(), "admin-alice", "correct-password").await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;
    let (status, body) = rescan(app.clone(), &cookie, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let axis_id = create_axis(
        app.clone(),
        &cookie,
        id,
        r#"{"name":"科目","source":"filenameWord","position":0}"#,
    )
    .await;
    put_axis_values(
        app.clone(),
        &cookie,
        id,
        axis_id,
        r#"{"values":[{"rawValue":"kokugo","displayName":"国語"},{"rawValue":"eigo","displayName":"英語"}]}"#,
    )
    .await;

    (app, cookie, id, dir)
}

/// 匿名は`authenticated`なアーカイブの一覧取得で401になる。
#[sqlx::test]
async fn archive_view_anonymous_on_authenticated_archive_is_unauthorized(pool: SqlitePool) {
    let dir = temp_test_dir("archive-view-401");

    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;

    // 既定は`public`なので、`authenticated`に変更してから検証する。
    let path_json = json_string(&dir.display().to_string());
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}"),
        &cookie,
        &format!(r#"{{"title":"英検 過去問","path":{path_json},"visibility":"authenticated"}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = view_archive(app, "", id, &[]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
}

/// アーカイブ以外のidは404。存在しないidと区別しない。
#[sqlx::test]
async fn archive_view_on_non_archive_content_is_not_found(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/contents",
        &cookie,
        r#"{"type":"link","title":"外部","url":"https://example.com"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let link_id = extract_id(&body);

    let (status, body) = view_archive(app.clone(), "", link_id, &[]).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = view_archive(app, "", 999999, &[]).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// 新しく見つかったアイテムは非公開から始まるため (→ docs/archive.md「スキャン」)、再スキャン直後の一覧は
/// 空になる。
#[sqlx::test]
async fn archive_view_returns_empty_before_publishing(pool: SqlitePool) {
    let (app, _cookie, id, _dir) = setup_archive_with_filename_word_axis(pool).await;

    let (status, body) = view_archive(app, "", id, &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    assert_eq!(parsed["items"], serde_json::json!([]), "{body}");
    assert_eq!(
        parsed["axes"][0]["options"],
        serde_json::json!([]),
        "{body}"
    );
}

/// `sort` クエリで並び順を選べる。並べ方の場合分けは `build_archive_view` の単体テストで見る。
/// ここは、クエリとスキャンで見つかった日時 (`archive_items.created_at`) がつながっていることだけ。
#[sqlx::test]
async fn archive_view_accepts_sort_query(pool: SqlitePool) {
    let (app, _cookie, id, _dir) = setup_archive_with_filename_word_axis(pool.clone()).await;
    publish_all_items(&pool, id).await;
    for (rel_path, created_at) in [
        ("2024_kokugo.pdf", "2026-09-01T00:00:00.000Z"),
        ("2024_eigo.pdf", "2026-09-20T00:00:00.000Z"),
        ("2024_unknown.pdf", "2026-09-10T00:00:00.000Z"),
    ] {
        sqlx::query!(
            "UPDATE archive_items SET created_at = ? WHERE archive_id = ? AND rel_path = ?",
            created_at,
            id,
            rel_path
        )
        .execute(&pool)
        .await
        .expect("created_atを更新できなかった");
    }

    let (status, body) = view_archive(app, "", id, &[("sort", "new")]).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    let file_names: Vec<&str> = parsed["items"]
        .as_array()
        .expect("itemsが配列でなかった")
        .iter()
        .map(|item| {
            item["fileName"]
                .as_str()
                .expect("fileNameが文字列でなかった")
        })
        .collect();
    assert_eq!(
        file_names,
        vec!["2024_eigo.pdf", "2024_unknown.pdf", "2024_kokugo.pdf"],
        "{body}"
    );
}

/// どの語にも一致しないアイテムは「未設定」として並び順の最後に来て、
/// タイトルはファイル名にフォールバックする (→ docs/archive.md「軸の値の辞書と導出」・「表示タイトル」)。
#[sqlx::test]
async fn archive_view_sorts_unset_last_and_falls_back_title(pool: SqlitePool) {
    let (app, cookie, id, dir) = setup_archive_with_filename_word_axis(pool.clone()).await;
    publish_all_items(&pool, id).await;

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}"),
        &cookie,
        &format!(
            r#"{{"title":"英検 過去問","path":{},"titleTemplate":"{{科目}}"}}"#,
            json_string(&dir.display().to_string())
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = view_archive(app, "", id, &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    let titles: Vec<&str> = parsed["items"]
        .as_array()
        .expect("itemsが配列でなかった")
        .iter()
        .map(|item| item["title"].as_str().unwrap())
        .collect();
    // 辞書のposition順(kokugo=0, eigo=1)、「未設定」(2024_unknown.pdf)は最後。
    assert_eq!(titles, vec!["国語", "英語", "2024_unknown.pdf"], "{body}");
}

/// 非公開アイテムは、ログイン済みの閲覧者に対してもダウンロードで404にする
/// (→ docs/archive.md「アイテムの公開」、docs/access.md「匿名閲覧の受け口」)。
#[sqlx::test]
async fn archive_download_item_hides_unpublished_even_when_logged_in(pool: SqlitePool) {
    let (app, cookie, id, _dir) = setup_archive_with_filename_word_axis(pool).await;

    let (status, body) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/items"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    let item_id = items[0]["id"].as_i64().expect("idが返らなかった");

    let (status, _) = send_empty(
        app,
        "GET",
        &format!("/api/v1/contents/{id}/items/{item_id}/download"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// 匿名は`authenticated`なアーカイブのアイテムダウンロードで401になる
/// (非公開の404とは区別する、→ docs/access.md「匿名閲覧の受け口」)。
#[sqlx::test]
async fn archive_download_item_anonymous_on_authenticated_archive_is_unauthorized(
    pool: SqlitePool,
) {
    let (app, cookie, id, dir) = setup_archive_with_filename_word_axis(pool.clone()).await;
    publish_all_items(&pool, id).await;

    let path_json = json_string(&dir.display().to_string());
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}"),
        &cookie,
        &format!(r#"{{"title":"英検 過去問","path":{path_json},"visibility":"authenticated"}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/items"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    let item_id = items[0]["id"].as_i64().expect("idが返らなかった");

    let (status, _) = send_anon(
        app,
        "GET",
        &format!("/api/v1/contents/{id}/items/{item_id}/download"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// スキャンは時点観測 (→ docs/archive.md「スキャン」)。索引に行があっても、走査後に実体が
/// 消えていれば配信は404にする。
#[sqlx::test]
async fn archive_download_item_returns_not_found_when_file_vanished_after_scan(pool: SqlitePool) {
    let (app, cookie, id, dir) = setup_archive_with_filename_word_axis(pool.clone()).await;
    publish_all_items(&pool, id).await;

    let (status, body) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/items"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    let item_id = items[0]["id"].as_i64().expect("idが返らなかった");
    let rel_path = items[0]["relPath"].as_str().expect("relPathが無かった");

    std::fs::remove_file(dir.join(rel_path)).expect("ファイルを消せなかった");

    let (status, _) = send_empty(
        app,
        "GET",
        &format!("/api/v1/contents/{id}/items/{item_id}/download"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// 公開済みアイテムは配信できる。
#[sqlx::test]
async fn archive_download_item_serves_published_file(pool: SqlitePool) {
    let (app, cookie, id, _dir) = setup_archive_with_filename_word_axis(pool.clone()).await;
    publish_all_items(&pool, id).await;

    let (status, body) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/items"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    let item_id = items[0]["id"].as_i64().expect("idが返らなかった");

    let (status, _) = send_empty(
        app,
        "GET",
        &format!("/api/v1/contents/{id}/items/{item_id}/download"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// 管理用の配信は、未公開のアイテムも渡す。匿名には401 (→ docs/archive.md「アイテムの配信」)。
#[sqlx::test]
async fn archive_manage_download_item_serves_unpublished_file(pool: SqlitePool) {
    let (app, cookie, id, _dir) = setup_archive_with_filename_word_axis(pool).await;

    let (status, body) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/items"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    assert_eq!(items[0]["published"], false, "{body}");
    let item_id = items[0]["id"].as_i64().expect("idが返らなかった");
    let uri = format!("/api/v1/contents/{id}/items/{item_id}/manage-download");

    let (status, body) = send_empty(app, "GET", &uri, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// 管理用の一覧は非公開も含めた全件を、導出済みタイトル付きで返す
/// (→ docs/archive.md「エンドポイント一覧」)。
#[sqlx::test]
async fn archive_list_items_includes_unpublished_with_derived_title(pool: SqlitePool) {
    let (app, cookie, id, _dir) = setup_archive_with_filename_word_axis(pool).await;

    let (status, body) =
        send_empty(app, "GET", &format!("/api/v1/contents/{id}/items"), &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    let items = items.as_array().expect("配列でなかった");
    assert_eq!(items.len(), 3, "{body}");
    // 新しく見つかったアイテムは非公開から始まる (→ docs/archive.md「スキャン」)。
    assert!(
        items.iter().all(|item| item["published"] == false),
        "{body}"
    );
    assert!(
        items
            .iter()
            .any(|item| item["relPath"] == "2024_kokugo.pdf" && item["title"] == "2024_kokugo.pdf"),
        "{body}"
    );
}

/// 管理用の一覧は、その時点のファイルの大きさと更新日時を付ける。
/// 索引の後に消えたファイルは空欄にし、一覧自体は返す。
#[sqlx::test]
async fn archive_list_items_reads_size_and_modified_at_from_files(pool: SqlitePool) {
    let (app, cookie, id, dir) = setup_archive_with_filename_word_axis(pool).await;
    std::fs::write(dir.join("2024_kokugo.pdf"), b"12345").expect("ファイルを書けなかった");
    std::fs::remove_file(dir.join("2024_eigo.pdf")).expect("ファイルを消せなかった");

    let (status, body) =
        send_empty(app, "GET", &format!("/api/v1/contents/{id}/items"), &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    let item = |rel_path: &str| {
        items
            .as_array()
            .expect("配列でなかった")
            .iter()
            .find(|item| item["relPath"] == rel_path)
            .unwrap_or_else(|| panic!("{rel_path} が無かった: {body}"))
            .clone()
    };
    let kokugo = item("2024_kokugo.pdf");
    assert_eq!(kokugo["size"], 5, "{body}");
    assert!(
        kokugo["modifiedAt"].as_i64().is_some_and(|ms| ms > 0),
        "{body}"
    );
    assert_eq!(kokugo["missing"], false, "{body}");
    let eigo = item("2024_eigo.pdf");
    assert!(
        eigo["missing"] == true && eigo["size"].is_null() && eigo["modifiedAt"].is_null(),
        "{body}"
    );
}

/// 閲覧の一覧は、索引の後に実体が消えた行を外す (→ docs/archive.md「アイテムの配信」)。
/// 登録先のフォルダごと無ければ、そう分かるように返す。
#[sqlx::test]
async fn archive_view_hides_items_whose_file_vanished_after_scan(pool: SqlitePool) {
    let (app, _cookie, id, dir) = setup_archive_with_filename_word_axis(pool.clone()).await;
    publish_all_items(&pool, id).await;
    std::fs::remove_file(dir.join("2024_eigo.pdf")).expect("ファイルを消せなかった");

    let (status, body) = view_archive(app.clone(), "", id, &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    let mut file_names: Vec<&str> = parsed["items"]
        .as_array()
        .expect("配列でなかった")
        .iter()
        .map(|item| item["fileName"].as_str().expect("fileNameが無かった"))
        .collect();
    file_names.sort_unstable();
    assert_eq!(
        file_names,
        vec!["2024_kokugo.pdf", "2024_unknown.pdf"],
        "{body}"
    );
    assert_eq!(parsed["folderMissing"], false, "{body}");

    std::fs::remove_dir_all(&*dir).expect("フォルダを消せなかった");
    let (status, body) = view_archive(app, "", id, &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    assert_eq!(parsed["items"], serde_json::json!([]), "{body}");
    assert_eq!(parsed["folderMissing"], true, "{body}");
}

/// データ置き場の祖先を指す folder でも、データ置き場の中は一覧にも配信にも出さない。
/// 一覧に出さないもの (ドット始まり・シンボリックリンク) も、パスを知っていても配信しない
/// (→ docs/archive.md「スキャン」・docs/folders.md「公開できるフォルダ」)。
#[sqlx::test]
async fn a_folder_never_serves_the_data_dir_or_hidden_entries(pool: SqlitePool) {
    let base = temp_test_dir("serve-hidden");
    let data_dir = base.join("weblav");
    std::fs::create_dir_all(data_dir.join("blobs")).expect("データ置き場を作れなかった");
    std::fs::write(data_dir.join("weblav.db"), b"secret").expect("ファイルを作れなかった");
    std::fs::create_dir_all(base.join(".git")).expect("フォルダを作れなかった");
    std::fs::write(base.join(".git").join("config"), b"x").expect("ファイルを作れなかった");
    std::fs::write(base.join(".env"), b"x").expect("ファイルを作れなかった");
    std::fs::write(base.join("a.mp3"), b"x").expect("ファイルを作れなかった");
    #[cfg(unix)]
    std::os::unix::fs::symlink(base.join("a.mp3"), base.join("link.mp3"))
        .expect("リンクを作れなかった");
    register_content_root(&pool).await;

    let app = app_in(pool.clone(), &Config::default(), &data_dir).await;
    sqlx::query(
        "INSERT INTO contents (type, title, path, visibility) VALUES ('folder', '祖先', ?, 'public')",
    )
    .bind(
        std::fs::canonicalize(&base)
            .expect("canonicalize できなかった")
            .display()
            .to_string(),
    )
    .execute(&pool)
    .await
    .expect("failed to insert folder content");

    let (status, body) = send_anon(app.clone(), "GET", "/api/v1/contents/1/browse?path=").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(entry_names(&body), vec!["a.mp3"], "{body}");

    let download = |path: &'static str| {
        let app = app.clone();
        async move {
            send_anon(
                app,
                "GET",
                &format!("/api/v1/contents/1/download?path={path}"),
            )
            .await
            .0
        }
    };
    assert_eq!(download("a.mp3").await, StatusCode::OK);
    assert_eq!(download("weblav/weblav.db").await, StatusCode::NOT_FOUND);
    assert_eq!(download(".env").await, StatusCode::NOT_FOUND);
    assert_eq!(download(".git/config").await, StatusCode::NOT_FOUND);
    #[cfg(unix)]
    assert_eq!(download("link.mp3").await, StatusCode::NOT_FOUND);
    // データ置き場そのものを辿ることもできない。
    let (status, body) = send_anon(app, "GET", "/api/v1/contents/1/browse?path=weblav").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// 自身の置き場 (設定とデータ) の祖先は「公開できるフォルダ」に登録できるが、置き場の中は登録できない。
#[sqlx::test]
async fn a_root_may_contain_own_dirs_but_not_be_inside_them(pool: SqlitePool) {
    let base = root_dir("roots-data-ancestor");
    let data_dir = base.join("weblav");
    std::fs::create_dir_all(data_dir.join("blobs")).expect("データ置き場を作れなかった");
    let config_dir = base.join("config");
    std::fs::create_dir_all(&config_dir).expect("設定の置き場を作れなかった");
    let app = build_app(
        pool.clone(),
        Key::generate(),
        &Config::default(),
        &AppDirs {
            config_dir: config_dir.clone(),
            data_dir: data_dir.clone(),
        },
        None,
        FolderPicker::new(|| Ok(None)),
    )
    .await
    .expect("failed to build app");
    let cookie = admin_cookie(app.clone(), &pool).await;
    let register = |dir: std::path::PathBuf| {
        let app = app.clone();
        let cookie = cookie.clone();
        async move {
            let path = canonical_path_as_api_returns_it(
                &std::fs::canonicalize(&dir).expect("canonicalize できなかった"),
            );
            send(
                app,
                roots_request("POST", ROOTS_URI, &cookie, Some(&json_path(&path))),
            )
            .await
        }
    };

    let (status, body) = register(data_dir.join("blobs")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.contains(r#""kind":"rootInsideOwnDirs""#), "{body}");
    let (status, body) = register(data_dir.clone()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, body) = register(config_dir.clone()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, body) = register(base.to_path_buf()).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // 登録先を選ぶ一覧に、設定とデータの置き場そのものが並ばない。
    std::fs::create_dir_all(base.join("other")).expect("フォルダを作れなかった");
    let uri = format!(
        "/api/v1/admin/fs/dirs?path={}",
        canonical_path_as_api_returns_it(&base)
    );
    let (status, body) = send(app.clone(), roots_request("GET", &uri, &cookie, None)).await;
    assert_eq!(status, StatusCode::OK, "{uri}: {body}");
    assert_eq!(entry_names(&body), vec!["other"], "{uri}: {body}");
}

/// 「公開できるフォルダ」の外を指す既存の行は、**配信の側でも**拒む (→ docs/folders.md「公開できるフォルダ」)。
///
/// 登録時の検証は書き込み経路にしか掛からないので、登録を消したあとの行や、データ置き場を
/// 移したあとの行が残りうる。ここでは「weblav 自身のデータ置き場を指す folder」をDBへ
/// 直接入れて再現する。
#[sqlx::test]
async fn browse_rejects_a_folder_pointing_at_the_data_dir(pool: SqlitePool) {
    let data_dir = temp_test_dir("browse-denied-root");
    let inside = data_dir.join("blobs");
    std::fs::create_dir_all(&inside).expect("データ置き場を作れなかった");
    std::fs::write(inside.join("secret.txt"), b"x").expect("ファイルを作れなかった");
    // データ置き場の隣は普通に見えること (規則を広げすぎていないことの確認)。
    let materials = test_support::TempDir::at(data_dir.parent().expect("親が無い").join(format!(
        "browse-denied-sibling-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    )));
    std::fs::write(materials.join("a.mp3"), b"x").expect("ファイルを作れなかった");
    register_content_root(&pool).await;

    let app = app_in(pool.clone(), &Config::default(), &data_dir).await;

    for (title, path) in [
        ("データ置き場", data_dir.display().to_string()),
        ("教材", materials.display().to_string()),
    ] {
        sqlx::query(
            "INSERT INTO contents (type, title, path, visibility) \
             VALUES ('folder', ?, ?, 'public')",
        )
        .bind(title)
        .bind(path)
        .execute(&pool)
        .await
        .expect("failed to insert folder content");
    }

    // 1件目 (データ置き場) は 404、2件目 (隣の教材フォルダ) は 200。
    let (status, body) = send_anon(app.clone(), "GET", "/api/v1/contents/1/browse?path=").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = send_anon(app.clone(), "GET", "/api/v1/contents/2/browse?path=").await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// 選択肢は表示名でまとめ、表示名で絞り込む。絞り込みに出さない軸は閲覧用 API に
/// 出さず、その軸名のクエリも無視する。並び順には使う (→ docs/archive.md「軸の定義」・「軸の値の辞書と導出」・「エンドポイント一覧」)。
#[sqlx::test]
async fn archive_view_groups_options_by_display_name_and_hides_unfilterable_axes(pool: SqlitePool) {
    let dir = temp_test_dir("archive-view-grouped");
    for (year, file) in [
        ("2024", "butsuri.pdf"),
        ("2024", "kokugo.pdf"),
        ("2023", "kagaku.pdf"),
    ] {
        std::fs::create_dir_all(dir.join(year)).expect("登録先を作れなかった");
        std::fs::write(dir.join(year).join(file), b"").expect("ファイルを作れなかった");
    }

    let (app, cookie, id) = setup_archive(&pool, &dir, None).await;
    publish_all_items(&pool, id).await;

    create_axis(
        app.clone(),
        &cookie,
        id,
        r#"{"name":"年度","source":"dirLevel","dirLevel":1,"filterable":false,"position":0}"#,
    )
    .await;
    let subject_axis_id = create_axis(
        app.clone(),
        &cookie,
        id,
        r#"{"name":"教科","source":"filenameWord","position":1}"#,
    )
    .await;
    put_axis_values(
        app.clone(),
        &cookie,
        id,
        subject_axis_id,
        r#"{"values":[{"rawValue":"kagaku","displayName":"理科"},{"rawValue":"kokugo","displayName":"国語"},{"rawValue":"butsuri","displayName":"理科"}]}"#,
    )
    .await;

    let (status, body) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/axes"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let axes: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    assert_eq!(axes[0]["filterable"], false, "{body}");
    assert_eq!(axes[1]["filterable"], true, "{body}");

    let (status, body) = view_archive(app.clone(), "", id, &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    assert_eq!(
        parsed["axes"],
        serde_json::json!([{
            "name": "教科",
            "options": [
                {"value": "理科", "count": 2},
                {"value": "国語", "count": 1},
            ],
        }]),
        "{body}"
    );
    // 絞り込みに出さない「年度」も並び順には効く (2023 が先)。
    let file_names: Vec<&str> = parsed["items"]
        .as_array()
        .expect("itemsが配列でなかった")
        .iter()
        .map(|item| item["fileName"].as_str().expect("fileNameが無かった"))
        .collect();
    assert_eq!(
        file_names,
        vec!["kagaku.pdf", "butsuri.pdf", "kokugo.pdf"],
        "{body}"
    );

    // 表示名で絞ると、その表示名にまとめた値のどれかに当たるアイテムが残る。
    let (status, body) = view_archive(app.clone(), "", id, &[("教科", "理科")]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    assert_eq!(
        parsed["items"]
            .as_array()
            .expect("itemsが配列でなかった")
            .len(),
        2,
        "{body}"
    );

    // 絞り込みに出さない軸の名前のキーは無視する。
    let (status, body) = view_archive(app, "", id, &[("年度", "2024")]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    assert_eq!(
        parsed["items"]
            .as_array()
            .expect("itemsが配列でなかった")
            .len(),
        3,
        "{body}"
    );
}

/// 一括切り替えは、所属しない`itemIds`が1件でもあれば全体を404にし、
/// どの行も更新しない (→ docs/archive.md「エンドポイント一覧」)。
#[sqlx::test]
async fn archive_bulk_publish_rejects_when_any_item_id_is_foreign(pool: SqlitePool) {
    let (app, cookie, id, _dir) = setup_archive_with_filename_word_axis(pool.clone()).await;

    let (status, body) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/items"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    let item_id = items[0]["id"].as_i64().expect("idが返らなかった");

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}/items"),
        &cookie,
        &format!(r#"{{"itemIds":[{item_id}, 999999],"published":true}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let published: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM archive_items WHERE archive_id = ? AND published = 1",
        id
    )
    .fetch_one(&pool)
    .await
    .expect("公開件数を数えられなかった");
    assert_eq!(
        published, 0,
        "全件が非公開のまま(ロールバックされている)であること"
    );
}

/// 一括切り替えの正常系。全件が対象アーカイブに属していれば全て更新する。
#[sqlx::test]
async fn archive_bulk_publish_updates_all_items(pool: SqlitePool) {
    let (app, cookie, id, _dir) = setup_archive_with_filename_word_axis(pool.clone()).await;

    let (status, body) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/items"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    let item_ids: Vec<i64> = items
        .as_array()
        .expect("配列でなかった")
        .iter()
        .map(|item| item["id"].as_i64().unwrap())
        .collect();

    let (status, body) = send_json(
        app,
        "PUT",
        &format!("/api/v1/contents/{id}/items"),
        &cookie,
        &format!(
            r#"{{"itemIds":{},"published":true}}"#,
            serde_json::to_string(&item_ids).unwrap()
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let published: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM archive_items WHERE archive_id = ? AND published = 1",
        id
    )
    .fetch_one(&pool)
    .await
    .expect("公開件数を数えられなかった");
    assert_eq!(published, 3);
}

/// 単体切り替え・配信は、所属しないitem_idに対して404を返す
/// (→ docs/archive.md「エンドポイント一覧」の所属条件)。
#[sqlx::test]
async fn archive_item_endpoints_404_for_item_belonging_to_another_archive(pool: SqlitePool) {
    let dir_a = temp_test_dir("archive-items-404-a");
    let dir_b = temp_test_dir("archive-items-404-b");
    std::fs::create_dir_all(&dir_a).expect("登録先を作れなかった");
    std::fs::create_dir_all(&dir_b).expect("登録先を作れなかった");
    std::fs::write(dir_a.join("a.mp3"), b"").expect("ファイルを作れなかった");

    let (app, cookie) = admin_app(&pool).await;
    let id_a = create_archive(app.clone(), &cookie, &dir_a, None).await;
    let id_b = create_archive(app.clone(), &cookie, &dir_b, None).await;
    let (status, body) = rescan(app.clone(), &cookie, id_a).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id_a}/items"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    let item_id = items[0]["id"].as_i64().expect("idが返らなかった");

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id_b}/items/{item_id}"),
        &cookie,
        r#"{"published":true}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, _) = send_empty(
        app,
        "GET",
        &format!("/api/v1/contents/{id_b}/items/{item_id}/download"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
