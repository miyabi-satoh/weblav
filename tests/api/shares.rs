//! 共有の中でできること (→ `api::shares`)

use super::*;

const SHARES_URI: &str = "/api/v1/shares";

fn access_uri(root_id: i64) -> String {
    format!("{ROOTS_URI}/{root_id}/access")
}

/// 共有の権限を管理者として置き換える。
async fn set_access(app: Router, cookie: &str, root_id: i64, json: &str) {
    let (status, body) = send_json(app, "PUT", &access_uri(root_id), cookie, json).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn root_id_of(pool: &SqlitePool, path: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM roots WHERE path = ?")
        .bind(path)
        .fetch_one(pool)
        .await
        .expect("共有の行を引けなかった")
}

/// テストの土台の共有 (→ `register_content_root`) の id。
async fn content_root_id(pool: &SqlitePool) -> i64 {
    let path = canonical_path_as_api_returns_it(
        &std::fs::canonicalize(content_root()).expect("canonicalize できなかった"),
    );
    root_id_of(pool, &path).await
}

/// 一覧の、名前ごとのできること。
fn accesses(body: &str) -> Vec<(String, String)> {
    parse_json(body)
        .as_array()
        .expect("配列が返るはず")
        .iter()
        .map(|share| {
            (
                share["name"].as_str().expect("名前").to_string(),
                share["access"].as_str().expect("できること").to_string(),
            )
        })
        .collect()
}

/// 見えない共有は一覧に出さない。入れ子では、含む共有のうち一番強いものになる。
#[sqlx::test]
async fn shares_list_the_strongest_access_and_leave_out_hidden_ones(pool: SqlitePool) {
    let (app, admin) = admin_app(&pool).await;
    insert_user(&pool, "taro", "password").await;
    let taro_id = db_user_id(&pool, "taro").await;
    let outer = root_dir("shares-nested");
    let inner = outer.join("inner");
    std::fs::create_dir_all(&inner).expect("フォルダーを作れなかった");
    let outer_path = canonical_path_as_api_returns_it(&outer);
    let inner_path = canonical_path_as_api_returns_it(&inner);
    register_root(&pool, &outer).await;
    register_root(&pool, &inner).await;
    let outer_id = root_id_of(&pool, &outer_path).await;
    let inner_id = root_id_of(&pool, &inner_path).await;
    let content_root = content_root_id(&pool).await;
    set_access(
        app.clone(),
        &admin,
        content_root,
        r#"{"editorAccess":"none","users":[]}"#,
    )
    .await;
    set_access(
        app.clone(),
        &admin,
        outer_id,
        r#"{"editorAccess":"none","users":[]}"#,
    )
    .await;
    set_access(
        app.clone(),
        &admin,
        inner_id,
        r#"{"editorAccess":"add","users":[]}"#,
    )
    .await;
    let taro = login(app.clone(), "taro", "password").await;

    let (status, body) = send_empty(app.clone(), "GET", SHARES_URI, &taro).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        accesses(&body),
        vec![(inner_path.clone(), "add".to_string())]
    );

    // 外側をこのユーザーだけ「すべて」にすると、内側も「すべて」になる。
    set_access(
        app.clone(),
        &admin,
        outer_id,
        &format!(r#"{{"editorAccess":"none","users":[{{"userId":{taro_id},"access":"all"}}]}}"#),
    )
    .await;
    let (status, body) = send_empty(app.clone(), "GET", SHARES_URI, &taro).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut expected = vec![
        (inner_path.clone(), "all".to_string()),
        (outer_path.clone(), "all".to_string()),
    ];
    expected.sort();
    let mut actual = accesses(&body);
    actual.sort();
    assert_eq!(actual, expected);

    // 管理者は、設定に関わらずすべての共有ですべての操作ができる。
    let (status, body) = send_empty(app, "GET", SHARES_URI, &admin).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let admin_accesses = accesses(&body);
    assert_eq!(admin_accesses.len(), 3, "{body}");
    assert!(
        admin_accesses.iter().all(|(_, access)| access == "all"),
        "{body}"
    );
}

/// 見えない共有の中は、登録先を選ぶ口でも外と同じに扱う。管理者は選べる。
#[sqlx::test]
async fn a_hidden_share_cannot_be_chosen_as_a_folder_location(pool: SqlitePool) {
    let (app, admin) = admin_app(&pool).await;
    insert_user(&pool, "taro", "password").await;
    let dir = temp_test_dir("shares-hidden-pick");
    let path = canonical_path_as_api_returns_it(
        &std::fs::canonicalize(&dir).expect("canonicalize できなかった"),
    );
    let root = canonical_path_as_api_returns_it(
        &std::fs::canonicalize(content_root()).expect("canonicalize できなかった"),
    );
    let root_id = content_root_id(&pool).await;
    set_access(
        app.clone(),
        &admin,
        root_id,
        r#"{"editorAccess":"none","users":[]}"#,
    )
    .await;
    let taro = login(app.clone(), "taro", "password").await;
    let encoded = utf8_percent_encode(&path, NON_ALPHANUMERIC).to_string();

    let (status, body) = send_empty(app.clone(), "GET", "/api/v1/admin/fs/dirs?path=", &taro).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains(&json_string(&root)), "{body}");
    for uri in [
        format!("/api/v1/admin/fs/dirs?path={encoded}"),
        format!("/api/v1/admin/fs/count?path={encoded}"),
    ] {
        let (status, body) = send_empty(app.clone(), "GET", &uri, &taro).await;
        assert_error_case(
            status,
            &body,
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request_body",
            &uri,
        );
    }
    let create = format!(
        r#"{{"type":"folder","title":"教材","path":{}}}"#,
        json_string(&path)
    );
    let (status, body) = send_json(app.clone(), "POST", "/api/v1/contents", &taro, &create).await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );

    let (status, body) = send_json(app, "POST", "/api/v1/contents", &admin, &create).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// 登録先を変えなければ、その共有が見えなくなった後でもほかの項目を直せる。
#[sqlx::test]
async fn a_folder_keeps_its_location_after_the_share_is_hidden(pool: SqlitePool) {
    let (app, admin) = admin_app(&pool).await;
    insert_user(&pool, "taro", "password").await;
    let dir = temp_test_dir("shares-keep-location");
    let path = canonical_path_as_api_returns_it(
        &std::fs::canonicalize(&dir).expect("canonicalize できなかった"),
    );
    let taro = login(app.clone(), "taro", "password").await;
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/contents",
        &taro,
        &format!(
            r#"{{"type":"folder","title":"教材","path":{}}}"#,
            json_string(&path)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = extract_id(&body);
    let root_id = content_root_id(&pool).await;
    set_access(
        app.clone(),
        &admin,
        root_id,
        r#"{"editorAccess":"none","users":[]}"#,
    )
    .await;

    let (status, body) = send_json(
        app,
        "PUT",
        &format!("/api/v1/contents/{id}"),
        &taro,
        &format!(r#"{{"title":"教材 2026","path":{}}}"#, json_string(&path)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// 共有を1つ用意し、`taro` (一般ユーザー) と管理者でログインしておく。
struct ShareFixture {
    app: Router,
    admin: String,
    taro: String,
    dir: test_support::TempDir,
    id: i64,
}

async fn share_fixture(pool: &SqlitePool, name: &str) -> ShareFixture {
    let (app, admin) = admin_app(pool).await;
    insert_user(pool, "taro", "password").await;
    let dir = root_dir(name);
    register_root(pool, &dir).await;
    let id = root_id_of(pool, &canonical_path_as_api_returns_it(&dir)).await;
    let taro = login(app.clone(), "taro", "password").await;
    ShareFixture {
        app,
        admin,
        taro,
        dir,
        id,
    }
}

fn browse_uri(id: i64, path: &str) -> String {
    format!(
        "/api/v1/shares/{id}/browse?path={}",
        utf8_percent_encode(path, NON_ALPHANUMERIC)
    )
}

fn entry_names(body: &serde_json::Value) -> Vec<String> {
    body["entries"]
        .as_array()
        .expect("entries は配列のはず")
        .iter()
        .map(|entry| entry["name"].as_str().expect("名前").to_string())
        .collect()
}

/// 中を一覧でき、ドット始まりは出さない。共有の既定 (「見る」) のできることが返る。
#[sqlx::test]
async fn a_share_can_be_browsed(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-browse").await;
    std::fs::create_dir_all(share.dir.join("教材")).expect("フォルダーを作れなかった");
    std::fs::create_dir_all(share.dir.join(".secret")).expect("フォルダーを作れなかった");
    std::fs::write(share.dir.join("教材").join("a.pdf"), b"pdf").expect("書けなかった");
    std::fs::write(share.dir.join("memo.txt"), b"memo").expect("書けなかった");
    std::fs::write(share.dir.join(".env"), b"x").expect("書けなかった");

    let (status, body) = send_empty(
        share.app.clone(),
        "GET",
        &browse_uri(share.id, ""),
        &share.taro,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let body = parse_json(&body);
    assert_eq!(body["access"], "read");
    assert_eq!(body["path"], "");
    assert_eq!(body["share"]["id"], share.id);
    assert_eq!(entry_names(&body), vec!["教材", "memo.txt"]);
    assert_eq!(body["canListOnHome"], true);
    // ごみ箱を使えるかは置き場所で決まるので、値ではなく出していることを見る。
    assert!(body["trash"].is_boolean(), "{body}");

    let (status, body) = send_empty(
        share.app.clone(),
        "GET",
        &browse_uri(share.id, "教材"),
        &share.taro,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let body = parse_json(&body);
    assert_eq!(body["path"], "教材");
    assert_eq!(entry_names(&body), vec!["a.pdf"]);

    // 管理者はすべての操作ができる。
    let (status, body) =
        send_empty(share.app, "GET", &browse_uri(share.id, ""), &share.admin).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse_json(&body)["access"], "all");
}

/// 共有の外へ出る道・一覧に出さないもの・見られない共有・ファイルは、どれも同じ 404。
#[sqlx::test]
async fn escapes_and_hidden_places_in_a_share_are_not_found(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-escapes").await;
    std::fs::create_dir_all(share.dir.join(".secret")).expect("フォルダーを作れなかった");
    std::fs::write(share.dir.join("memo.txt"), b"memo").expect("書けなかった");
    let outside = temp_root_dir("shares-escapes-outside");
    std::fs::create_dir_all(&outside).expect("フォルダーを作れなかった");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, share.dir.join("link")).expect("リンクを作れなかった");

    let mut paths = vec![
        "..",
        "../shares-escapes-outside",
        ".secret",
        "missing",
        "memo.txt",
    ];
    if cfg!(unix) {
        paths.push("link");
    }
    for path in paths {
        let uri = browse_uri(share.id, path);
        let (status, body) = send_empty(share.app.clone(), "GET", &uri, &share.taro).await;
        assert_error_case(status, &body, StatusCode::NOT_FOUND, "not_found", &uri);
    }
    let (status, body) =
        send_empty(share.app.clone(), "GET", &browse_uri(9999, ""), &share.taro).await;
    assert_error(status, &body, StatusCode::NOT_FOUND, "not_found");

    set_access(
        share.app.clone(),
        &share.admin,
        share.id,
        r#"{"editorAccess":"none","users":[]}"#,
    )
    .await;
    for uri in [
        browse_uri(share.id, ""),
        format!("/api/v1/shares/{}/download?path=memo.txt", share.id),
        format!("/api/v1/shares/{}/dirs?path=", share.id),
    ] {
        let (status, body) = send_empty(share.app.clone(), "GET", &uri, &share.taro).await;
        assert_error_case(status, &body, StatusCode::NOT_FOUND, "not_found", &uri);
    }
}

/// ファイルはダウンロードでき、フォルダーはダウンロードできない。
#[sqlx::test]
async fn a_file_in_a_share_can_be_downloaded(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-download").await;
    std::fs::create_dir_all(share.dir.join("教材")).expect("フォルダーを作れなかった");
    std::fs::write(share.dir.join("教材").join("memo.txt"), b"memo").expect("書けなかった");
    let uri = |path: &str| {
        format!(
            "/api/v1/shares/{}/download?path={}",
            share.id,
            utf8_percent_encode(path, NON_ALPHANUMERIC)
        )
    };

    let (status, body) =
        send_empty(share.app.clone(), "GET", &uri("教材/memo.txt"), &share.taro).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, "memo");
    let (status, body) = send_empty(share.app, "GET", &uri("教材"), &share.taro).await;
    assert_error(status, &body, StatusCode::NOT_FOUND, "not_found");
}

/// フォルダーの選択には、フォルダーだけが出る。
#[sqlx::test]
async fn share_dirs_list_only_folders(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-dirs").await;
    std::fs::create_dir_all(share.dir.join("b").join("c")).expect("フォルダーを作れなかった");
    std::fs::create_dir_all(share.dir.join("a")).expect("フォルダーを作れなかった");
    std::fs::write(share.dir.join("memo.txt"), b"memo").expect("書けなかった");

    let (status, body) = send_empty(
        share.app.clone(),
        "GET",
        &format!("/api/v1/shares/{}/dirs?path=", share.id),
        &share.taro,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let body = parse_json(&body);
    assert_eq!(body["parent"], serde_json::Value::Null);
    assert_eq!(
        body["entries"],
        serde_json::json!([{"name": "a", "path": "a"}, {"name": "b", "path": "b"}])
    );
    let (status, body) = send_empty(
        share.app,
        "GET",
        &format!("/api/v1/shares/{}/dirs?path=b", share.id),
        &share.taro,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let body = parse_json(&body);
    assert_eq!(body["parent"], "");
    assert_eq!(
        body["entries"],
        serde_json::json!([{"name": "c", "path": "b/c"}])
    );
}

/// フォルダーとして載せている場所には、載せているコンテンツが付く。他人の「作成者のみ」は出さない。
#[sqlx::test]
async fn listed_folders_are_marked_in_a_share(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-listed").await;
    std::fs::create_dir_all(share.dir.join("教材")).expect("フォルダーを作れなかった");
    std::fs::create_dir_all(share.dir.join("私物")).expect("フォルダーを作れなかった");
    let admin_id = db_user_id(&pool, "admin-alice").await;
    let listed = insert_folder_by(
        &pool,
        "英検の教材",
        std::fs::canonicalize(share.dir.join("教材")).expect("canonicalize できなかった"),
        "authenticated",
        Some(admin_id),
        None,
    )
    .await;
    insert_folder_by(
        &pool,
        "先生の私物",
        std::fs::canonicalize(share.dir.join("私物")).expect("canonicalize できなかった"),
        "private",
        Some(admin_id),
        None,
    )
    .await;

    let (status, body) = send_empty(
        share.app.clone(),
        "GET",
        &browse_uri(share.id, ""),
        &share.taro,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let body = parse_json(&body);
    let entries = body["entries"].as_array().expect("配列のはず");
    assert_eq!(
        entries[0]["listedAs"],
        serde_json::json!({"contentId": listed, "title": "英検の教材"})
    );
    assert_eq!(entries[1]["listedAs"], serde_json::Value::Null);

    let (status, body) = send_empty(
        share.app.clone(),
        "GET",
        &browse_uri(share.id, "教材"),
        &share.taro,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let body = parse_json(&body);
    assert_eq!(body["listedAs"]["contentId"], listed);
    assert_eq!(body["canListOnHome"], false);

    // 管理者には、他人の「作成者のみ」も出る。
    let (status, body) = send_empty(
        share.app,
        "GET",
        &browse_uri(share.id, "私物"),
        &share.admin,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse_json(&body)["listedAs"]["title"], "先生の私物");
}

/// フォルダーのコンテンツの画面には、見られる共有の中の場所が付く。匿名と、見られない人には付かない。
#[sqlx::test]
async fn a_folder_content_points_to_its_share(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-folder-link").await;
    std::fs::create_dir_all(share.dir.join("教材").join("英検")).expect("フォルダーを作れなかった");
    let id = insert_folder_by(
        &pool,
        "教材",
        std::fs::canonicalize(share.dir.join("教材")).expect("canonicalize できなかった"),
        "public",
        None,
        None,
    )
    .await;
    let uri = format!(
        "/api/v1/contents/{id}/browse?path={}",
        utf8_percent_encode("英検", NON_ALPHANUMERIC)
    );

    let (status, body) = send_empty(share.app.clone(), "GET", &uri, &share.taro).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut found = parse_json(&body)["share"].clone();
    // ごみ箱を使えるかは置き場所で決まるので、値ではなく出していることを見る。
    let trash = found
        .as_object_mut()
        .expect("share が付くはず")
        .remove("trash");
    assert!(trash.is_some_and(|trash| trash.is_boolean()), "{body}");
    assert_eq!(
        found,
        serde_json::json!({
            "id": share.id,
            "name": canonical_path_as_api_returns_it(&share.dir),
            "path": "教材/英検",
            "access": "read",
        })
    );

    let (status, body) = send_anon(share.app.clone(), "GET", &uri).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse_json(&body)["share"], serde_json::Value::Null);

    set_access(
        share.app.clone(),
        &share.admin,
        share.id,
        r#"{"editorAccess":"none","users":[]}"#,
    )
    .await;
    let (status, body) = send_empty(share.app, "GET", &uri, &share.taro).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse_json(&body)["share"], serde_json::Value::Null);
}
