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

/// 見えない共有は一覧に出さない。入れ子の決め方 (含む共有のうち一番強いもの) は `AccessMap::at_share` の単体テストで確かめる。
#[sqlx::test]
async fn shares_list_the_strongest_access_and_leave_out_hidden_ones(pool: SqlitePool) {
    let (app, admin) = admin_app(&pool).await;
    insert_user(&pool, "taro", "password").await;
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

    let (status, body) = send_empty(app.clone(), "GET", "/api/v1/fs/dirs?path=", &taro).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains(&json_string(&root)), "{body}");
    for uri in [
        format!("/api/v1/fs/dirs?path={encoded}"),
        format!("/api/v1/fs/count?path={encoded}"),
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
pub(crate) struct ShareFixture {
    pub(crate) app: Router,
    pub(crate) admin: String,
    pub(crate) taro: String,
    pub(crate) dir: test_support::TempDir,
    pub(crate) id: i64,
}

pub(crate) async fn share_fixture(pool: &SqlitePool, name: &str) -> ShareFixture {
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

pub(crate) fn browse_uri(id: i64, path: &str) -> String {
    format!(
        "/api/v1/shares/{id}/browse?path={}",
        utf8_percent_encode(path, NON_ALPHANUMERIC)
    )
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
    assert_eq!(entry_names_of(&body), vec!["教材", "memo.txt"]);
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
    assert_eq!(entry_names_of(&body), vec!["a.pdf"]);

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

    // 無いもの・リンクで外へ出るものは、解決の単体テスト (`shares::paths`) で場合ごとに確かめる。
    // ここでは、`..` (解決の前に弾く)・一覧に出さないもの・ファイルが同じ 404 につながることを見る。
    for path in ["..", "../shares-escapes-outside", ".secret", "memo.txt"] {
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

/// 「非表示」しか載せていない場所は掲載中にせず、また載せられる。
#[sqlx::test]
async fn a_folder_with_only_a_hidden_content_can_be_listed_again(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-listed-hidden").await;
    std::fs::create_dir_all(share.dir.join("教材")).expect("フォルダーを作れなかった");
    insert_folder_by(
        &pool,
        "隠した教材",
        std::fs::canonicalize(share.dir.join("教材")).expect("canonicalize できなかった"),
        "hidden",
        None,
        None,
    )
    .await;

    let (status, body) = send_empty(
        share.app.clone(),
        "GET",
        &browse_uri(share.id, "教材"),
        &share.taro,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let body = parse_json(&body);
    assert_eq!(body["listedAs"], serde_json::Value::Null);
    assert_eq!(body["canListOnHome"], true);

    let (status, body) = send_json(
        share.app,
        "POST",
        &format!("/api/v1/shares/{}/list-on-home", share.id),
        &share.taro,
        r#"{"path":"教材"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// 自分で戻せる非表示のコンテンツがあるフォルダーを載せると、新しく作らずにそれを戻す。
#[sqlx::test]
async fn listing_a_folder_you_hid_brings_that_content_back(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-list-restores").await;
    std::fs::create_dir_all(share.dir.join("教材")).expect("フォルダーを作れなかった");
    let taro_id = db_user_id(&pool, "taro").await;
    let hidden = insert_folder_by(
        &pool,
        "隠した教材",
        std::fs::canonicalize(share.dir.join("教材")).expect("canonicalize できなかった"),
        "hidden",
        Some(taro_id),
        None,
    )
    .await;

    let (status, body) = send_json(
        share.app,
        "POST",
        &format!("/api/v1/shares/{}/list-on-home", share.id),
        &share.taro,
        r#"{"path":"教材","visibility":"authenticated"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let body = parse_json(&body);
    assert_eq!(body["id"], hidden);
    assert_eq!(body["title"], "隠した教材");
    assert_eq!(body["visibility"], "authenticated");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contents")
        .fetch_one(&pool)
        .await
        .expect("数えられなかった");
    assert_eq!(count, 1, "新しく作らない");
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

/// `files` を `file` の欄に並べた multipart の要求。
fn upload_request(uri: &str, cookie: &str, files: &[(&str, &[u8])]) -> Request<Body> {
    let fields: Vec<(&str, &str, &[u8])> = files
        .iter()
        .map(|&(name, content)| ("file", name, content))
        .collect();
    multipart_request("POST", uri, cookie, &[], &fields)
}

fn upload_uri(id: i64, path: &str) -> String {
    format!(
        "/api/v1/shares/{id}/upload?path={}",
        utf8_percent_encode(path, NON_ALPHANUMERIC)
    )
}

/// 共有の既定を変える。
pub(crate) async fn set_editor_access(share: &ShareFixture, access: &str) {
    set_access(
        share.app.clone(),
        &share.admin,
        share.id,
        &format!(r#"{{"editorAccess":"{access}","users":[]}}"#),
    )
    .await;
}

/// 先頭だけ JPEG の印を持つ中身 (→ `fsops::sniff_image`)。
const JPEG_BYTES: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F'];

/// いくつかのファイルを1回で置ける。同じ名前は番号を付け、中身が JPEG の `.HEIC` は `.jpg` にする。
/// 置いたことを記録に残す。
#[sqlx::test]
async fn files_can_be_uploaded_into_a_share(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-upload").await;
    set_editor_access(&share, "add").await;
    std::fs::create_dir_all(share.dir.join("教材")).expect("フォルダーを作れなかった");
    std::fs::write(share.dir.join("教材").join("memo.txt"), b"old").expect("書けなかった");

    let request = upload_request(
        &upload_uri(share.id, "教材"),
        &share.taro,
        &[
            ("memo.txt", b"new"),
            ("IMG_0001.HEIC", JPEG_BYTES),
            ("a.pdf", b"pdf"),
        ],
    );
    let (status, body) = send(share.app.clone(), request).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        parse_json(&body)["files"],
        serde_json::json!([
            {"name": "memo (1).txt", "renamed": true, "extensionFixed": false},
            {"name": "IMG_0001.jpg", "renamed": false, "extensionFixed": true},
            {"name": "a.pdf", "renamed": false, "extensionFixed": false},
        ])
    );
    let dir = share.dir.join("教材");
    assert_eq!(
        std::fs::read(dir.join("memo.txt")).expect("読めなかった"),
        b"old"
    );
    assert_eq!(
        std::fs::read(dir.join("memo (1).txt")).expect("読めなかった"),
        b"new"
    );
    assert!(dir.join("IMG_0001.jpg").is_file());
    let leftovers: Vec<String> = std::fs::read_dir(&dir)
        .expect("読めなかった")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with('.'))
        .collect();
    assert!(
        leftovers.is_empty(),
        "作業用のファイルが残った: {leftovers:?}"
    );

    let logged: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT op, paths, result FROM operations WHERE root_id IS NOT NULL ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .expect("記録を読めなかった");
    assert_eq!(
        logged,
        vec![
            (
                "upload".to_string(),
                r#"["教材/memo (1).txt"]"#.to_string(),
                "succeeded".to_string()
            ),
            (
                "upload".to_string(),
                r#"["教材/IMG_0001.jpg"]"#.to_string(),
                "succeeded".to_string()
            ),
            (
                "upload".to_string(),
                r#"["教材/a.pdf"]"#.to_string(),
                "succeeded".to_string()
            ),
        ]
    );
}

/// 見るだけの人は足せない (403)。見えない場所・一覧に出さない場所は 404。
#[sqlx::test]
async fn uploading_needs_add_access_to_a_visible_folder(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-upload-denied").await;
    std::fs::create_dir_all(share.dir.join(".secret")).expect("フォルダーを作れなかった");

    let request = upload_request(&upload_uri(share.id, ""), &share.taro, &[("a.pdf", b"pdf")]);
    let (status, body) = send(share.app.clone(), request).await;
    assert_error(status, &body, StatusCode::FORBIDDEN, "forbidden");

    set_editor_access(&share, "add").await;
    let request = upload_request(
        &upload_uri(share.id, ".secret"),
        &share.taro,
        &[("a.pdf", b"pdf")],
    );
    let (status, body) = send(share.app.clone(), request).await;
    assert_error(status, &body, StatusCode::NOT_FOUND, "not_found");

    set_editor_access(&share, "none").await;
    let request = upload_request(&upload_uri(share.id, ""), &share.taro, &[("a.pdf", b"pdf")]);
    let (status, body) = send(share.app.clone(), request).await;
    assert_error(status, &body, StatusCode::NOT_FOUND, "not_found");
    assert!(!share.dir.join("a.pdf").exists());
}

/// 1つのファイルの上限を超えたら 413 にし、途中まで受け取ったものは残さない。
#[sqlx::test]
async fn an_upload_over_the_limit_is_rejected(pool: SqlitePool) {
    let mut config = Config::default();
    config.upload.max_size_mb = Some(1);
    let app = app_in(pool.clone(), &config, &next_app_dir()).await;
    let admin = admin_cookie(app.clone(), &pool).await;
    let dir = root_dir("shares-upload-limit");
    register_root(&pool, &dir).await;
    let id = root_id_of(&pool, &canonical_path_as_api_returns_it(&dir)).await;

    let content = vec![b'x'; 1024 * 1024 + 1];
    let request = upload_request(&upload_uri(id, ""), &admin, &[("big.bin", &content)]);
    let (status, body) = send(app, request).await;

    assert_error(
        status,
        &body,
        StatusCode::PAYLOAD_TOO_LARGE,
        "file_too_large",
    );
    let left: Vec<_> = std::fs::read_dir(&*dir)
        .expect("読めなかった")
        .filter_map(|entry| entry.ok())
        .collect();
    assert!(left.is_empty(), "何も残さないはず: {left:?}");
}

/// 上限なし (既定) なら、axum の既定の要求の大きさの上限 (2MB) を超えるファイルも受け取る。
#[sqlx::test]
async fn an_upload_without_a_limit_accepts_large_files(pool: SqlitePool) {
    let app = app_in(pool.clone(), &Config::default(), &next_app_dir()).await;
    let admin = admin_cookie(app.clone(), &pool).await;
    let dir = root_dir("shares-upload-unlimited");
    register_root(&pool, &dir).await;
    let id = root_id_of(&pool, &canonical_path_as_api_returns_it(&dir)).await;

    let content = vec![b'x'; 3 * 1024 * 1024];
    let request = upload_request(&upload_uri(id, ""), &admin, &[("big.bin", &content)]);
    let (status, body) = send(app, request).await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    let size = std::fs::metadata(dir.join("big.bin"))
        .expect("置いたファイルが無い")
        .len();
    assert_eq!(size, content.len() as u64);
}

/// 置いたファイルは、次の再スキャンを待たずに一覧の表示の索引に入る。
#[sqlx::test]
async fn an_uploaded_file_enters_the_list_view_index(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-upload-index").await;
    std::fs::write(share.dir.join("a.pdf"), b"pdf").expect("書けなかった");
    let archive_id = create_archive(share.app.clone(), &share.admin, &share.dir, None).await;
    let (status, body) = rescan(share.app.clone(), &share.admin, archive_id).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let request = upload_request(
        &upload_uri(share.id, ""),
        &share.admin,
        &[("b.pdf", b"pdf")],
    );
    let (status, body) = send(share.app.clone(), request).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // 索引は裏で読み直すので、入るまで待つ。
    let mut indexed = Vec::new();
    for _ in 0..100 {
        indexed = sqlx::query_scalar::<_, String>(
            "SELECT rel_path FROM archive_items WHERE archive_id = ? ORDER BY rel_path",
        )
        .bind(archive_id)
        .fetch_all(&pool)
        .await
        .expect("索引を読めなかった");
        if indexed.len() == 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(indexed, vec!["a.pdf", "b.pdf"]);
}

pub(crate) fn json_uri(id: i64, action: &str) -> String {
    format!("/api/v1/shares/{id}/{action}")
}

/// 新しいフォルダーを作れる。同じ名前があれば 422 (`entryNameTaken`)、使えない名前は `entryNameInvalid`。
#[sqlx::test]
async fn a_folder_can_be_created_in_a_share(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-mkdir").await;
    let uri = json_uri(share.id, "folders");
    let request = r#"{"path":"","name":"教材"}"#;

    let (status, body) = send_json(share.app.clone(), "POST", &uri, &share.taro, request).await;
    assert_error(status, &body, StatusCode::FORBIDDEN, "forbidden");

    set_editor_access(&share, "add").await;
    let (status, body) = send_json(share.app.clone(), "POST", &uri, &share.taro, request).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(parse_json(&body)["path"], "教材");
    assert!(share.dir.join("教材").is_dir());

    let (status, body) = send_json(share.app.clone(), "POST", &uri, &share.taro, request).await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
    assert_eq!(
        parse_json(&body)["error"]["detail"]["kind"],
        "entryNameTaken"
    );

    let (status, body) = send_json(
        share.app.clone(),
        "POST",
        &uri,
        &share.taro,
        r#"{"path":"","name":"a/b"}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
    assert_eq!(
        parse_json(&body)["error"]["detail"]["kind"],
        "entryNameInvalid"
    );
}

/// 名前の変更は「すべて」の人だけ。変えると、中のものの索引の行が付いていく。
/// 共有そのものは変えられない。
#[sqlx::test]
async fn renaming_needs_full_access_and_carries_the_index_along(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-rename").await;
    std::fs::create_dir_all(share.dir.join("教材")).expect("フォルダーを作れなかった");
    std::fs::write(share.dir.join("教材").join("a.pdf"), b"pdf").expect("書けなかった");
    let folder = insert_folder_by(&pool, "共有", &*share.dir, "public", None, None).await;
    sqlx::query("INSERT INTO archive_items (archive_id, rel_path) VALUES (?, '教材/a.pdf')")
        .bind(folder)
        .execute(&pool)
        .await
        .expect("索引を入れられなかった");
    let uri = json_uri(share.id, "rename");
    let request = r#"{"path":"教材","name":"教材 2026"}"#;

    set_editor_access(&share, "add").await;
    let (status, body) = send_json(share.app.clone(), "POST", &uri, &share.taro, request).await;
    assert_error(status, &body, StatusCode::FORBIDDEN, "forbidden");

    set_editor_access(&share, "all").await;
    let (status, body) = send_json(share.app.clone(), "POST", &uri, &share.taro, request).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse_json(&body)["path"], "教材 2026");
    assert!(share.dir.join("教材 2026").join("a.pdf").is_file());
    let rels: Vec<String> =
        sqlx::query_scalar("SELECT rel_path FROM archive_items WHERE archive_id = ?")
            .bind(folder)
            .fetch_all(&pool)
            .await
            .expect("索引を読めなかった");
    assert_eq!(rels, vec!["教材 2026/a.pdf"]);

    let (status, body) = send_json(
        share.app.clone(),
        "POST",
        &uri,
        &share.taro,
        r#"{"path":"","name":"別の名前"}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
    assert_eq!(
        parse_json(&body)["error"]["detail"]["kind"],
        "entryProtected"
    );
}

/// 中に別の共有があるフォルダーは、名前を変えられない。
#[sqlx::test]
async fn a_folder_holding_another_share_cannot_be_renamed(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-rename-nested").await;
    let inner = share.dir.join("教材").join("英検");
    std::fs::create_dir_all(&inner).expect("フォルダーを作れなかった");
    register_root(&pool, &inner).await;

    let (status, body) = send_json(
        share.app.clone(),
        "POST",
        &json_uri(share.id, "rename"),
        &share.admin,
        r#"{"path":"教材","name":"教材 2026"}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
    assert_eq!(
        parse_json(&body)["error"]["detail"]["kind"],
        "entryProtected"
    );
    assert!(inner.is_dir());
}

/// 見られる人は、フォルダーをホームに載せられる。載せると共有の一覧に「掲載中」が付く。
#[sqlx::test]
async fn a_share_folder_can_be_listed_on_home(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-list-on-home").await;
    std::fs::create_dir_all(share.dir.join("教材")).expect("フォルダーを作れなかった");

    let (status, body) = send_json(
        share.app.clone(),
        "POST",
        &json_uri(share.id, "list-on-home"),
        &share.taro,
        r#"{"path":"教材","visibility":"authenticated"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let created = parse_json(&body);
    assert_eq!(created["title"], "教材");
    assert_eq!(created["visibility"], "authenticated");

    let (status, body) = send_empty(
        share.app.clone(),
        "GET",
        &browse_uri(share.id, "教材"),
        &share.taro,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let body = parse_json(&body);
    assert_eq!(body["listedAs"]["contentId"], created["id"]);
    assert_eq!(body["canListOnHome"], false);
}

fn transfer_json(
    op: &str,
    from: i64,
    paths: &[&str],
    to: i64,
    path: &str,
    on_conflict: Option<&str>,
) -> String {
    let mut value = serde_json::json!({
        "op": op,
        "from": {"shareId": from, "paths": paths},
        "to": {"shareId": to, "path": path},
    });
    if let Some(on_conflict) = on_conflict {
        value["onConflict"] = serde_json::json!(on_conflict);
    }
    value.to_string()
}

const TRANSFER_URI: &str = "/api/v1/shares/transfer";
const TRANSFER_CHECK_URI: &str = "/api/v1/shares/transfer/check";

/// コピーの前に、置き先の同じ名前と大きさが分かる。番号を付けて写すと、元も置き先の前のものも残る。
/// 同じフォルダーの中へのコピーは、同じ名前とみなさずに番号を付けて写す。
#[sqlx::test]
async fn copying_reports_conflicts_and_can_add_a_number(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-copy").await;
    set_editor_access(&share, "add").await;
    std::fs::create_dir_all(share.dir.join("教材").join("英検")).expect("フォルダーを作れなかった");
    std::fs::write(share.dir.join("教材").join("英検").join("a.pdf"), b"12345")
        .expect("書けなかった");
    std::fs::create_dir_all(share.dir.join("控え").join("英検")).expect("フォルダーを作れなかった");
    std::fs::write(share.dir.join("memo.txt"), b"memo").expect("書けなかった");

    let check = transfer_json("copy", share.id, &["教材/英検"], share.id, "控え", None);
    let (status, body) = send_json(
        share.app.clone(),
        "POST",
        TRANSFER_CHECK_URI,
        &share.taro,
        &check,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let body = parse_json(&body);
    assert_eq!(
        body["conflicts"],
        serde_json::json!([{"name": "英検", "isDir": true}])
    );
    assert_eq!(body["totalBytes"], 5);
    assert!(body["availableBytes"].as_u64().is_some(), "{body}");
    assert!(
        body["trash"].is_boolean(),
        "上書きの確認で、元に戻せるかを出すため: {body}"
    );

    let start = transfer_json(
        "copy",
        share.id,
        &["教材/英検"],
        share.id,
        "控え",
        Some("rename"),
    );
    let (status, body) =
        send_json(share.app.clone(), "POST", TRANSFER_URI, &share.taro, &start).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let job = wait_job(share.app.clone(), &share.taro, &body).await;
    assert_eq!(job["state"], "succeeded", "{job}");
    assert_eq!(job["op"], "copy");
    assert_eq!(job["doneBytes"], 5);
    assert!(
        share
            .dir
            .join("控え")
            .join("英検 (1)")
            .join("a.pdf")
            .is_file()
    );
    assert!(share.dir.join("教材").join("英検").join("a.pdf").is_file());

    let (status, body) = send_json(
        share.app.clone(),
        "POST",
        TRANSFER_CHECK_URI,
        &share.taro,
        &transfer_json("copy", share.id, &["memo.txt"], share.id, "", None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse_json(&body)["conflicts"], serde_json::json!([]));
    let (status, body) = send_json(
        share.app.clone(),
        "POST",
        TRANSFER_URI,
        &share.taro,
        &transfer_json("copy", share.id, &["memo.txt"], share.id, "", Some("skip")),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let job = wait_job(share.app.clone(), &share.taro, &body).await;
    assert_eq!(job["state"], "succeeded", "{job}");
    assert!(share.dir.join("memo (1).txt").is_file());

    let logged: Vec<(String, String, Option<i64>)> = sqlx::query_as(
        "SELECT op, result, item_count FROM operations WHERE root_id IS NOT NULL ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .expect("記録を読めなかった");
    assert_eq!(
        logged,
        vec![
            ("copy".to_string(), "succeeded".to_string(), Some(2)),
            ("copy".to_string(), "succeeded".to_string(), Some(1)),
        ]
    );
}

/// 「追加できる」人は、写して入れられるが、動かせず、上書きもできない。
#[sqlx::test]
async fn add_access_allows_copying_in_but_not_moving_or_overwriting(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-add-only").await;
    set_editor_access(&share, "add").await;
    std::fs::create_dir_all(share.dir.join("控え")).expect("フォルダーを作れなかった");
    std::fs::write(share.dir.join("a.pdf"), b"new").expect("書けなかった");
    std::fs::write(share.dir.join("控え").join("a.pdf"), b"old").expect("書けなかった");

    for json in [
        transfer_json(
            "move",
            share.id,
            &["a.pdf"],
            share.id,
            "控え",
            Some("rename"),
        ),
        transfer_json(
            "copy",
            share.id,
            &["a.pdf"],
            share.id,
            "控え",
            Some("overwrite"),
        ),
    ] {
        let (status, body) =
            send_json(share.app.clone(), "POST", TRANSFER_URI, &share.taro, &json).await;
        assert_error_case(status, &body, StatusCode::FORBIDDEN, "forbidden", &json);
    }
    assert_eq!(
        std::fs::read(share.dir.join("控え").join("a.pdf")).expect("読めなかった"),
        b"old"
    );

    // 見るだけでは置けない。
    set_editor_access(&share, "read").await;
    let json = transfer_json(
        "copy",
        share.id,
        &["a.pdf"],
        share.id,
        "控え",
        Some("rename"),
    );
    let (status, body) =
        send_json(share.app.clone(), "POST", TRANSFER_URI, &share.taro, &json).await;
    assert_error(status, &body, StatusCode::FORBIDDEN, "forbidden");
}

/// 別の共有へ動かすと、元の共有を載せたフォルダーの印は外れ、置き先を載せたフォルダーの印は残る。
/// 上書きでは、置き先の前のものはごみ箱へ送る。
#[sqlx::test]
async fn moving_between_shares_overwrites_and_clears_the_index(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-move-from").await;
    let dest_dir = root_dir("shares-move-to");
    register_root(&pool, &dest_dir).await;
    let dest_id = root_id_of(&pool, &canonical_path_as_api_returns_it(&dest_dir)).await;
    std::fs::create_dir_all(share.dir.join("教材")).expect("フォルダーを作れなかった");
    std::fs::write(share.dir.join("教材").join("a.pdf"), b"new").expect("書けなかった");
    std::fs::create_dir_all(dest_dir.join("教材")).expect("フォルダーを作れなかった");
    std::fs::write(dest_dir.join("教材").join("old.pdf"), b"old").expect("書けなかった");
    let from_folder = insert_folder_by(&pool, "元", &*share.dir, "public", None, None).await;
    let to_folder = insert_folder_by(&pool, "先", &*dest_dir, "public", None, None).await;
    for (content, path) in [(from_folder, "教材/a.pdf"), (to_folder, "教材/old.pdf")] {
        sqlx::query("INSERT INTO archive_items (archive_id, rel_path) VALUES (?, ?)")
            .bind(content)
            .bind(path)
            .execute(&pool)
            .await
            .expect("索引を入れられなかった");
    }

    let json = transfer_json("move", share.id, &["教材"], dest_id, "", Some("overwrite"));
    let (status, body) =
        send_json(share.app.clone(), "POST", TRANSFER_URI, &share.admin, &json).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let job = wait_job(share.app.clone(), &share.admin, &body).await;
    assert_eq!(job["state"], "succeeded", "{job}");

    assert!(!share.dir.join("教材").exists());
    assert_eq!(
        std::fs::read(dest_dir.join("教材").join("a.pdf")).expect("読めなかった"),
        b"new"
    );
    assert!(!dest_dir.join("教材").join("old.pdf").exists());
    // 動かした元の行はコンテンツの外へ出たので消え、置き先の行は読み直させるので消える。
    let rels: Vec<String> = sqlx::query_scalar("SELECT rel_path FROM archive_items")
        .fetch_all(&pool)
        .await
        .expect("索引を読めなかった");
    assert!(rels.is_empty(), "{rels:?}");
}

/// 上書きの前に、響くコンテンツが分かる。上書きすると、そのコンテンツの索引が消え、消したものが削除として記録に残る。
#[sqlx::test]
async fn overwriting_a_listed_folder_warns_clears_the_index_and_records_the_deletion(
    pool: SqlitePool,
) {
    let share = share_fixture(&pool, "shares-overwrite-listed").await;
    std::fs::create_dir_all(share.dir.join("新").join("教材")).expect("フォルダーを作れなかった");
    std::fs::write(share.dir.join("新").join("教材").join("a.pdf"), b"new").expect("書けなかった");
    std::fs::create_dir_all(share.dir.join("教材")).expect("フォルダーを作れなかった");
    std::fs::write(share.dir.join("教材").join("a.pdf"), b"old").expect("書けなかった");
    let listed = insert_folder_by(
        &pool,
        "英検",
        std::fs::canonicalize(share.dir.join("教材")).expect("canonicalize できなかった"),
        "public",
        None,
        None,
    )
    .await;
    sqlx::query("INSERT INTO archive_items (archive_id, rel_path) VALUES (?, 'a.pdf')")
        .bind(listed)
        .execute(&pool)
        .await
        .expect("索引を入れられなかった");

    let (status, body) = send_json(
        share.app.clone(),
        "POST",
        TRANSFER_CHECK_URI,
        &share.admin,
        &transfer_json("copy", share.id, &["新/教材"], share.id, "", None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        parse_json(&body)["listed"],
        serde_json::json!([{"contentId": listed, "title": "英検"}])
    );

    let json = transfer_json(
        "copy",
        share.id,
        &["新/教材"],
        share.id,
        "",
        Some("overwrite"),
    );
    let (status, body) =
        send_json(share.app.clone(), "POST", TRANSFER_URI, &share.admin, &json).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let job = wait_job(share.app.clone(), &share.admin, &body).await;
    assert_eq!(job["state"], "succeeded", "{job}");
    assert_eq!(
        std::fs::read(share.dir.join("教材").join("a.pdf")).expect("読めなかった"),
        b"new"
    );

    let rels: Vec<String> = sqlx::query_scalar("SELECT rel_path FROM archive_items")
        .fetch_all(&pool)
        .await
        .expect("索引を読めなかった");
    assert!(rels.is_empty(), "{rels:?}");
    let logged: Vec<(String, String, Option<i64>)> = sqlx::query_as(
        "SELECT op, paths, item_count FROM operations WHERE root_id IS NOT NULL ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .expect("記録を読めなかった");
    assert_eq!(
        logged,
        vec![
            ("copy".to_string(), r#"["新/教材"]"#.to_string(), Some(2)),
            ("delete".to_string(), r#"["教材"]"#.to_string(), Some(1)),
        ]
    );
}

/// 飛ばしたものは、進み具合では全体まで進むが、記録には数えない。
#[sqlx::test]
async fn a_skipped_transfer_reaches_the_total_but_records_nothing_done(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-skip-log").await;
    std::fs::create_dir_all(share.dir.join("控え")).expect("フォルダーを作れなかった");
    std::fs::write(share.dir.join("a.pdf"), b"new").expect("書けなかった");
    std::fs::write(share.dir.join("控え").join("a.pdf"), b"old").expect("書けなかった");

    let json = transfer_json("copy", share.id, &["a.pdf"], share.id, "控え", Some("skip"));
    let (status, body) =
        send_json(share.app.clone(), "POST", TRANSFER_URI, &share.admin, &json).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let job = wait_job(share.app.clone(), &share.admin, &body).await;
    assert_eq!(job["skipped"], 1, "{job}");
    assert_eq!(job["doneItems"], job["totalItems"], "{job}");
    assert_eq!(job["doneBytes"], 3, "{job}");

    let logged: (i64, i64) =
        sqlx::query_as("SELECT item_count, byte_count FROM operations WHERE root_id IS NOT NULL")
            .fetch_one(&pool)
            .await
            .expect("記録を読めなかった");
    assert_eq!(logged, (0, 0));
}

/// フォルダーを、自分の中へは動かせない。
#[sqlx::test]
async fn a_folder_cannot_be_moved_into_itself(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-move-into").await;
    std::fs::create_dir_all(share.dir.join("教材").join("英検")).expect("フォルダーを作れなかった");
    let json = transfer_json(
        "move",
        share.id,
        &["教材"],
        share.id,
        "教材/英検",
        Some("rename"),
    );
    let (status, body) =
        send_json(share.app.clone(), "POST", TRANSFER_URI, &share.admin, &json).await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
    assert_eq!(
        parse_json(&body)["error"]["detail"]["kind"],
        "transferIntoItself"
    );
}

/// 消す前に、ごみ箱へ送れるかと、消すと開けなくなるコンテンツが分かる。消すとごみ箱へ送る。
#[sqlx::test]
async fn deleting_reports_listed_folders_and_sends_entries_to_the_trash(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-delete").await;
    std::fs::create_dir_all(share.dir.join("教材").join("英検")).expect("フォルダーを作れなかった");
    std::fs::write(share.dir.join("memo.txt"), b"memo").expect("書けなかった");
    let listed = insert_folder_by(
        &pool,
        "英検",
        share.dir.join("教材").join("英検"),
        "public",
        None,
        None,
    )
    .await;
    let check_uri = json_uri(share.id, "delete/check");
    let uri = json_uri(share.id, "delete");
    let paths = r#"{"paths":["教材","memo.txt"]}"#;

    let (status, body) = send_json(share.app.clone(), "POST", &check_uri, &share.taro, paths).await;
    assert_error(status, &body, StatusCode::FORBIDDEN, "forbidden");

    set_editor_access(&share, "all").await;
    let (status, body) = send_json(share.app.clone(), "POST", &check_uri, &share.taro, paths).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let body = parse_json(&body);
    assert_eq!(body["trash"], true);
    assert_eq!(
        body["listed"],
        serde_json::json!([{"contentId": listed, "title": "英検"}])
    );

    let (status, body) = send_json(share.app.clone(), "POST", &uri, &share.taro, paths).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let job = wait_job(share.app.clone(), &share.taro, &body).await;
    assert_eq!(job["state"], "succeeded", "{job}");
    assert_eq!(job["doneItems"], 2);
    assert!(!share.dir.join("教材").exists());
    assert!(!share.dir.join("memo.txt").exists());
    // 登録先を消しても、コンテンツは残す (ごみ箱から戻せば開ける)。
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contents WHERE id = ?")
        .bind(listed)
        .fetch_one(&pool)
        .await
        .expect("数えられなかった");
    assert_eq!(left, 1);

    // 共有そのものは消せない。
    let (status, body) = send_json(
        share.app.clone(),
        "POST",
        &uri,
        &share.taro,
        r#"{"paths":[""]}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
    assert_eq!(
        parse_json(&body)["error"]["detail"]["kind"],
        "entryProtected"
    );
}

/// ごみ箱の無い場所では、ごみ箱へ送る削除を断り、完全に消す削除だけを受ける。
#[sqlx::test]
async fn without_a_trash_only_a_permanent_delete_is_accepted(pool: SqlitePool) {
    register_content_root(&pool).await;
    let data_dir = next_app_dir();
    let app = build_app(
        pool.clone(),
        Key::generate(),
        &Config::default(),
        &AppDirs {
            config_dir: data_dir.clone(),
            data_dir,
        },
        None,
        FolderPicker::new(|| Ok(None)),
        TrashPolicy::Unavailable,
    )
    .await
    .expect("failed to build app");
    let admin = admin_cookie(app.clone(), &pool).await;
    let dir = root_dir("shares-no-trash");
    register_root(&pool, &dir).await;
    let id = root_id_of(&pool, &canonical_path_as_api_returns_it(&dir)).await;
    std::fs::write(dir.join("a.pdf"), b"pdf").expect("書けなかった");

    let (status, body) = send_json(
        app.clone(),
        "POST",
        &json_uri(id, "delete/check"),
        &admin,
        r#"{"paths":["a.pdf"]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse_json(&body)["trash"], false);
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &json_uri(id, "delete"),
        &admin,
        r#"{"paths":["a.pdf"]}"#,
    )
    .await;
    assert_error(status, &body, StatusCode::CONFLICT, "trash_unavailable");
    assert!(dir.join("a.pdf").exists());

    let (status, body) = send_json(
        app.clone(),
        "POST",
        &json_uri(id, "delete"),
        &admin,
        r#"{"paths":["a.pdf"],"permanent":true}"#,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let job = wait_job(app, &admin, &body).await;
    assert_eq!(job["state"], "succeeded", "{job}");
    assert!(!dir.join("a.pdf").exists());
}

/// 処理は始めた人だけが見られ、中止できる。
#[sqlx::test]
async fn a_job_belongs_to_the_user_who_started_it(pool: SqlitePool) {
    let share = share_fixture(&pool, "shares-job-owner").await;
    std::fs::write(share.dir.join("a.pdf"), b"pdf").expect("書けなかった");
    let json = transfer_json("copy", share.id, &["a.pdf"], share.id, "", Some("rename"));
    let (status, body) =
        send_json(share.app.clone(), "POST", TRANSFER_URI, &share.admin, &json).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let id = parse_json(&body)["jobId"]
        .as_str()
        .expect("jobId")
        .to_string();
    let uri = format!("/api/v1/shares/jobs/{id}");

    let (status, body) = send_empty(share.app.clone(), "GET", &uri, &share.taro).await;
    assert_error(status, &body, StatusCode::NOT_FOUND, "not_found");
    let (status, body) = send_empty(share.app.clone(), "DELETE", &uri, &share.taro).await;
    assert_error(status, &body, StatusCode::NOT_FOUND, "not_found");
    let (status, body) =
        send_empty(share.app.clone(), "GET", "/api/v1/shares/jobs", &share.taro).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse_json(&body), serde_json::json!([]));

    let (status, body) = send_empty(share.app.clone(), "DELETE", &uri, &share.admin).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, body) = send_empty(
        share.app.clone(),
        "GET",
        "/api/v1/shares/jobs",
        &share.admin,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse_json(&body)[0]["id"], id);
}
