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
