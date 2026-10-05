//! 公開できるフォルダ (→ docs/folders.md「公開できるフォルダ」)

use super::*;

#[sqlx::test]
async fn roots_can_be_registered_listed_and_deleted(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    let dir = root_dir("roots-crud");
    let path = canonical_path_as_api_returns_it(&dir);

    let (status, body) = send(
        app.clone(),
        roots_request("POST", ROOTS_URI, &cookie, Some(&json_path(&path))),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(body.contains(&json_string(&path)), "{body}");
    // 名前を省くとフォルダ名になる。
    let folder_name = dir
        .file_name()
        .expect("フォルダ名があるはず")
        .to_string_lossy()
        .into_owned();
    assert!(
        body.contains(&format!(r#""name":{}"#, json_string(&folder_name))),
        "{body}"
    );

    let (status, body) = send(app.clone(), roots_request("GET", ROOTS_URI, &cookie, None)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(&json_string(&path)), "{body}");

    // テストの土台の登録 (→ `register_content_root`) も入っているので、パスで引く。
    let id: i64 = sqlx::query_scalar!(r#"SELECT id as "id!" FROM roots WHERE path = ?"#, path)
        .fetch_one(&pool)
        .await
        .expect("登録した行を読めなかった");
    let (status, body) = send(
        app.clone(),
        roots_request("DELETE", &format!("{ROOTS_URI}/{id}"), &cookie, None),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (status, body) = send(app, roots_request("GET", ROOTS_URI, &cookie, None)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains(&json_string(&path)), "{body}");

    // 行は削除済みの印を付けて残る (→ docs/folders.md「公開できるフォルダ」)。
    let deleted: bool = sqlx::query_scalar!(
        r#"SELECT deleted_at IS NOT NULL as "deleted!: bool" FROM roots WHERE id = ?"#,
        id
    )
    .fetch_one(&pool)
    .await
    .expect("削除した行が残っていない");
    assert!(deleted);
}

/// 名前を変えられる。空ならフォルダ名に戻り、他と重なる名前は拒む。削除済みの名前は使える。
#[sqlx::test]
async fn a_root_can_be_renamed(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    let base = temp_root_dir("roots-rename");
    let first = base.join("a");
    let second = base.join("b");
    std::fs::create_dir_all(&first).expect("フォルダを作れなかった");
    std::fs::create_dir_all(&second).expect("フォルダを作れなかった");
    let register = |dir: std::path::PathBuf, name: &'static str| {
        let app = app.clone();
        let cookie = cookie.clone();
        async move {
            let body = json_path_and_name(
                &canonical_path_as_api_returns_it(
                    &std::fs::canonicalize(&dir).expect("canonicalize できなかった"),
                ),
                name,
            );
            let (status, body) =
                send(app, roots_request("POST", ROOTS_URI, &cookie, Some(&body))).await;
            assert_eq!(status, StatusCode::CREATED, "{body}");
            extract_id(&body)
        }
    };
    let first_id = register(first.clone(), "教材").await;
    let second_id = register(second.clone(), "資料").await;
    let rename = |id: i64, name: &'static str| {
        let app = app.clone();
        let cookie = cookie.clone();
        async move {
            send(
                app,
                roots_request(
                    "PUT",
                    &format!("{ROOTS_URI}/{id}"),
                    &cookie,
                    Some(&format!(r#"{{"name":{}}}"#, json_string(name))),
                ),
            )
            .await
        }
    };

    let (status, body) = rename(first_id, " 教材2 ").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""name":"教材2""#), "{body}");

    let (status, body) = rename(first_id, "資料").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.contains(r#""kind":"rootNameTaken""#), "{body}");

    // 自分自身の名前のままでもよい。
    let (status, body) = rename(second_id, "資料").await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = rename(first_id, "").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""name":"a""#), "{body}");

    // 削除済みの名前は、削除済みを除いて見るので使える。
    let (status, body) = send(
        app.clone(),
        roots_request("DELETE", &format!("{ROOTS_URI}/{second_id}"), &cookie, None),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, body) = rename(first_id, "資料").await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // 削除済みは名前を変えられない。
    let (status, body) = rename(second_id, "別名").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// 削除済みのフォルダは、判定から外れ、中を辿れない
/// (行は場所の表示のためだけに残す → docs/folders.md「公開できるフォルダ」)。
#[sqlx::test]
async fn a_deleted_root_is_left_out_of_every_check(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    let dir = root_dir("roots-deleted-checks");
    let path = canonical_path_as_api_returns_it(&dir);

    let (status, created) = send(
        app.clone(),
        roots_request("POST", ROOTS_URI, &cookie, Some(&json_path(&path))),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = extract_id(&created);
    let (status, body) = send(
        app.clone(),
        roots_request("DELETE", &format!("{ROOTS_URI}/{id}"), &cookie, None),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    // ピッカーの最初の一覧に出ず、中も辿れない。
    let dirs_uri = |query: &str| format!("/api/v1/admin/fs/dirs?path={query}");
    let (status, body) = send_empty(app.clone(), "GET", &dirs_uri(""), &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains(&json_string(&path)), "{body}");
    let (status, body) = send_empty(app.clone(), "GET", &dirs_uri(&path), &cookie).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

/// 削除済みのパスを登録し直すと、同じ行が戻り、新しい名前になる。
#[sqlx::test]
async fn re_registering_a_deleted_root_restores_the_same_row(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    let dir = root_dir("roots-restore");
    let path = canonical_path_as_api_returns_it(&dir);
    let body = |name: &str| json_path_and_name(&path, name);

    let (status, created) = send(
        app.clone(),
        roots_request("POST", ROOTS_URI, &cookie, Some(&body("前の名前"))),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = extract_id(&created);
    let (status, deleted) = send(
        app.clone(),
        roots_request("DELETE", &format!("{ROOTS_URI}/{id}"), &cookie, None),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{deleted}");

    let (status, restored) = send(
        app,
        roots_request("POST", ROOTS_URI, &cookie, Some(&body("新しい名前"))),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{restored}");
    assert_eq!(extract_id(&restored), id);
    assert!(restored.contains(r#""name":"新しい名前""#), "{restored}");
}

const ROOTS_PICK_URI: &str = "/api/v1/admin/roots/pick";

/// 窓で選ばれたパスを、登録に渡せる形で返す。キャンセルなら `null`。登録はしない。
#[sqlx::test]
async fn picking_a_root_returns_the_chosen_path(pool: SqlitePool) {
    let dir = root_dir("roots-pick");
    let picked = dir.to_path_buf();
    let roots_pick_data = temp_test_dir("roots-pick-data");
    let app = app_with_picker(
        pool.clone(),
        &Config::default(),
        &roots_pick_data,
        FolderPicker::new(move || Ok(Some(picked.clone()))),
    )
    .await;
    let cookie = admin_cookie(app.clone(), &pool).await;

    let (status, body) = send(
        app.clone(),
        roots_request("POST", ROOTS_PICK_URI, &cookie, None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        format!(
            r#"{{"path":{}}}"#,
            json_string(&canonical_path_as_api_returns_it(&dir))
        )
    );
    let (_, list) = send(app, roots_request("GET", ROOTS_URI, &cookie, None)).await;
    assert!(
        !list.contains(&json_string(&canonical_path_as_api_returns_it(&dir))),
        "{list}"
    );

    let roots_pick_cancel_data = temp_test_dir("roots-pick-cancel-data");
    let app = app_with_picker(
        pool.clone(),
        &Config::default(),
        &roots_pick_cancel_data,
        FolderPicker::new(|| Ok(None)),
    )
    .await;
    // アプリごとにセッション鍵が違うので、ログインし直す。
    let cookie = login(app.clone(), "admin-alice", "correct-password").await;
    let (status, body) = send(app, roots_request("POST", ROOTS_PICK_URI, &cookie, None)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, r#"{"path":null}"#);
}

/// macOS では、窓で選んだフォルダのブックマークを残す (→ docs/distribution.md「ビルド・配布の方法」)。
/// 同じフォルダを選び直したら書き直し、登録したフォルダの分は起動時に戻せる。
#[cfg(target_os = "macos")]
#[sqlx::test]
async fn picking_a_root_keeps_a_bookmark_on_macos(pool: SqlitePool) {
    let dir = root_dir("roots-pick-bookmark");
    let picked = dir.to_path_buf();
    let data = temp_test_dir("roots-pick-bookmark-data");
    let app = app_with_picker(
        pool.clone(),
        &Config::default(),
        &data,
        FolderPicker::new(move || Ok(Some(picked.clone()))),
    )
    .await;
    let cookie = admin_cookie(app.clone(), &pool).await;
    let path = canonical_path_as_api_returns_it(&dir);

    for _ in 0..2 {
        let (status, body) = send(
            app.clone(),
            roots_request("POST", ROOTS_PICK_URI, &cookie, None),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let paths: Vec<String> = sqlx::query_scalar("SELECT path FROM folder_bookmarks")
        .fetch_all(&pool)
        .await
        .expect("ブックマークを読めなかった");
    assert_eq!(paths, vec![path.clone()]);

    let register = format!(r#"{{"path":{}}}"#, json_string(&path));
    let (status, body) = send(
        app,
        roots_request("POST", ROOTS_URI, &cookie, Some(&register)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    weblav::folder_access::restore(&pool).await;
}

/// 窓は同時に1つだけ。開いている間の2つ目は 409 で、閉じたらまた開ける。
#[sqlx::test]
async fn only_one_root_picker_opens_at_a_time(pool: SqlitePool) {
    let (opened_tx, opened_rx) = std::sync::mpsc::channel::<()>();
    let (close_tx, close_rx) = std::sync::mpsc::channel::<()>();
    let close_rx = std::sync::Mutex::new(close_rx);
    let roots_pick_busy_data = temp_test_dir("roots-pick-busy-data");
    let app = app_with_picker(
        pool.clone(),
        &Config::default(),
        &roots_pick_busy_data,
        FolderPicker::new(move || {
            let _ = opened_tx.send(());
            let _ = close_rx.lock().expect("鍵が毒された").recv();
            Ok(None)
        }),
    )
    .await;
    let cookie = admin_cookie(app.clone(), &pool).await;

    let first = tokio::spawn(send(
        app.clone(),
        roots_request("POST", ROOTS_PICK_URI, &cookie, None),
    ));
    tokio::task::spawn_blocking(move || opened_rx.recv().expect("窓が開かなかった"))
        .await
        .expect("待てなかった");

    let (status, body) = send(
        app.clone(),
        roots_request("POST", ROOTS_PICK_URI, &cookie, None),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    close_tx.send(()).expect("窓を閉じられなかった");
    let (status, body) = first.await.expect("最初の要求が落ちた");
    assert_eq!(status, StatusCode::OK, "{body}");

    close_tx.send(()).expect("窓を閉じられなかった");
    let (status, body) = send(app, roots_request("POST", ROOTS_PICK_URI, &cookie, None)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// 名前は前後の空白を除いて保存し、同じ名前 (大文字小文字だけの違いも含む) は拒む。
/// 名前がフルパスの代わりに見分ける手掛かりになるため (→ docs/folders.md「公開できるフォルダ」)。
#[sqlx::test]
async fn a_root_name_is_trimmed_and_must_be_unique(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    let base = temp_root_dir("roots-names");
    let first = base.join("a");
    let second = base.join("b");
    std::fs::create_dir_all(&first).expect("フォルダを作れなかった");
    std::fs::create_dir_all(&second).expect("フォルダを作れなかった");
    let request_body = |dir: &std::path::Path, name: &str| {
        json_path_and_name(
            &canonical_path_as_api_returns_it(
                &std::fs::canonicalize(dir).expect("canonicalize できなかった"),
            ),
            name,
        )
    };

    let (status, body) = send(
        app.clone(),
        roots_request(
            "POST",
            ROOTS_URI,
            &cookie,
            Some(&request_body(&first, "  Materials ")),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(body.contains(r#""name":"Materials""#), "{body}");

    let (status, body) = send(
        app.clone(),
        roots_request(
            "POST",
            ROOTS_URI,
            &cookie,
            Some(&request_body(&second, "materials")),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.contains(r#""kind":"rootNameTaken""#), "{body}");

    // 空の名前はフォルダ名になる。
    let (status, body) = send(
        app,
        roots_request(
            "POST",
            ROOTS_URI,
            &cookie,
            Some(&request_body(&second, " ")),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(body.contains(r#""name":"b""#), "{body}");
}

/// 管理画面のコンテンツ一覧から、`id` の行を取り出す。
async fn admin_content_by_id(app: Router, cookie: &str, id: i64) -> serde_json::Value {
    let (status, body) = send_empty(app, "GET", "/api/v1/admin/contents", cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let body: serde_json::Value = serde_json::from_str(&body).expect("応答が JSON でない");
    body.as_array()
        .expect("配列でない")
        .iter()
        .find(|content| content["id"] == id)
        .expect("作ったアーカイブが無い")
        .clone()
}

/// 管理画面のコンテンツ一覧は、起点のフルパスの代わりに出す名前と、そこから先のパスを返す。
#[sqlx::test]
async fn admin_contents_locate_a_path_by_its_root_name(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    let parent = temp_test_dir("root-location");
    let dir = parent.join("英検");
    std::fs::create_dir_all(&dir).expect("フォルダを作れなかった");
    let dir = std::fs::canonicalize(&dir).expect("canonicalize できなかった");
    let id = create_archive(app.clone(), &cookie, &dir, None).await;
    // `register_root` は名前をパスと同じにするので、パスを返しても通ってしまう。違う名前にしておく。
    let root = canonical_path_as_api_returns_it(
        &std::fs::canonicalize(content_root()).expect("canonicalize できなかった"),
    );
    sqlx::query("UPDATE roots SET name = '教材' WHERE path = ?")
        .bind(&root)
        .execute(&pool)
        .await
        .expect("名前を変えられなかった");

    let content = admin_content_by_id(app.clone(), &cookie, id).await;
    let thread_dir = dir
        .parent()
        .and_then(|parent| parent.file_name())
        .expect("親フォルダ名があるはず")
        .to_string_lossy()
        .into_owned();
    assert_eq!(content["rootName"], "教材", "{content}");
    assert_eq!(
        content["pathInRoot"],
        format!("{thread_dir}/英検"),
        "{content}"
    );
    assert_eq!(content["rootDeleted"], false, "{content}");

    // 登録を削除しても、どこまでが起点だったかは残り、削除済みとして返す。
    sqlx::query("UPDATE roots SET deleted_at = 'x' WHERE path = ?")
        .bind(&root)
        .execute(&pool)
        .await
        .expect("削除済みにできなかった");
    let content = admin_content_by_id(app.clone(), &cookie, id).await;
    assert_eq!(content["rootDeleted"], true, "{content}");
    assert_eq!(
        content["pathInRoot"],
        format!("{thread_dir}/英検"),
        "{content}"
    );
}

/// 入れ子は登録できる。同じフォルダの二重登録だけを拒み、登録済みの名前を返す (→ docs/folders.md「公開できるフォルダ」)。
#[sqlx::test]
async fn a_root_can_be_nested_but_not_registered_twice(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    let parent = root_dir("roots-nested");
    let child = parent.join("child");
    std::fs::create_dir_all(&child).expect("フォルダを作れなかった");
    let post = |path: &std::path::Path| {
        roots_request(
            "POST",
            ROOTS_URI,
            &cookie,
            Some(&json_path(&canonical_path_as_api_returns_it(path))),
        )
    };

    let (status, body) = send(app.clone(), post(&parent)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, body) = send(app.clone(), post(&child)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, body) = send(app, post(&child)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.contains(r#""kind":"rootAlreadyRegistered""#), "{body}");
    assert!(body.contains(r#""name":"child""#), "{body}");
}

/// 入れ子では、外側を起点にする。外側から内側へ辿っても、「上へ」で外側へ戻れる (→ docs/folders.md「公開できるフォルダ」)。
#[sqlx::test]
async fn browsing_into_a_nested_root_keeps_the_outer_one_as_the_start(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    let outer = root_dir("roots-nested-browse");
    let inner = outer.join("inner");
    std::fs::create_dir_all(&inner).expect("フォルダを作れなかった");
    let outer_path = canonical_path_as_api_returns_it(&outer);
    let inner_path = canonical_path_as_api_returns_it(&inner);
    for path in [&outer_path, &inner_path] {
        let (status, body) = send(
            app.clone(),
            roots_request("POST", ROOTS_URI, &cookie, Some(&json_path(path))),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }

    let uri = format!(
        "/api/v1/admin/fs/dirs?path={}",
        utf8_percent_encode(&inner_path, NON_ALPHANUMERIC)
    );
    let (status, body) = send_empty(app, "GET", &uri, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let listing: serde_json::Value = serde_json::from_str(&body).expect("JSON のはず");
    assert_eq!(
        listing["root"].as_str(),
        Some(outer_path.as_str()),
        "{body}"
    );
    assert_eq!(
        listing["parent"].as_str(),
        Some(outer_path.as_str()),
        "{body}"
    );
}

/// 登録し直すと、中に残っていたコンテンツの件数が戻る。削除でコンテンツの登録は
/// 消していないため (→ docs/folders.md「公開できるフォルダ」)。
#[sqlx::test]
async fn re_registering_a_root_reports_the_contents_left_inside(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    let dir = root_dir("roots-recount");
    let path = canonical_path_as_api_returns_it(&dir);

    let (status, body) = send(
        app.clone(),
        roots_request("POST", ROOTS_URI, &cookie, Some(&json_path(&path))),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(body.contains(r#""contentCount":0"#), "{body}");
    let id = extract_id(&body);

    create_archive(app.clone(), &cookie, &dir, None).await;

    let (status, body) = send(
        app.clone(),
        roots_request("DELETE", &format!("{ROOTS_URI}/{id}"), &cookie, None),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (status, body) = send(
        app,
        roots_request("POST", ROOTS_URI, &cookie, Some(&json_path(&path))),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(body.contains(r#""contentCount":1"#), "{body}");
}

#[sqlx::test]
async fn a_root_that_does_not_exist_is_rejected(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    let missing = temp_root_path("roots-missing");

    let (status, body) = send(
        app,
        roots_request(
            "POST",
            ROOTS_URI,
            &cookie,
            Some(&json_path(&missing.display().to_string())),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

/// 名前を付けて登録する要求の本文。
fn json_path_and_name(path: &str, name: &str) -> String {
    format!(
        r#"{{"path":{},"name":{}}}"#,
        json_string(path),
        json_string(name)
    )
}
