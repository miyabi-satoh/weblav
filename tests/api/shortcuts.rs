//! ファイルを指すショートカット (→ docs/folders.md「ショートカット」)。

use super::*;

/// 共有の中のファイルを指すショートカットを作り、id を返す。
async fn create_shortcut(
    app: Router,
    cookie: &str,
    path: &std::path::Path,
    visibility: &str,
) -> i64 {
    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/contents",
        cookie,
        &format!(
            r#"{{"type":"shortcut","path":{},"visibility":"{visibility}"}}"#,
            json_string(&path.display().to_string())
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    extract_id(&body)
}

/// 土台の共有の id。
async fn root_id_for(pool: &SqlitePool, _dir: &std::path::Path) -> i64 {
    let root = canonical_path_as_api_returns_it(
        &std::fs::canonicalize(content_root()).expect("canonicalize できなかった"),
    );
    sqlx::query_scalar("SELECT id FROM roots WHERE path = ?")
        .bind(root)
        .fetch_one(pool)
        .await
        .expect("土台の共有があるはず")
}

/// `dir` の中の `name` の、土台の共有からの相対パス。`name` が空なら `dir` そのもの。
async fn rel_in_share(_pool: &SqlitePool, dir: &std::path::Path, name: &str) -> String {
    let root = std::fs::canonicalize(content_root()).expect("canonicalize できなかった");
    let dir = std::fs::canonicalize(dir).expect("canonicalize できなかった");
    let rel = dir
        .strip_prefix(root)
        .expect("土台の共有の中のはず")
        .to_string_lossy()
        .replace('\\', "/");
    if name.is_empty() {
        rel
    } else {
        format!("{rel}/{name}")
    }
}

/// 閲覧の一覧の行。
async fn home_row(app: Router, cookie: &str, id: i64) -> serde_json::Value {
    let (status, body) = send_empty(app, "GET", "/api/v1/contents", cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    parse_json(&body)
        .as_array()
        .expect("配列のはず")
        .iter()
        .find(|row| row["id"] == id)
        .cloned()
        .expect("一覧に無かった")
}

/// 閲覧の一覧にはファイルと同じ形で並び、id で開ける。タイトルはファイル名になる。
#[sqlx::test]
async fn a_shortcut_appears_as_a_file_and_opens_by_id(pool: SqlitePool) {
    let dir = temp_test_dir("shortcut-open");
    let file = dir.join("過去問.txt");
    std::fs::write(&file, b"hello").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;

    let id = create_shortcut(app.clone(), &cookie, &file, "public").await;

    let row = home_row(app.clone(), &cookie, id).await;
    assert_eq!(row["type"], "file", "{row}");
    assert_eq!(row["title"], "過去問.txt", "{row}");
    assert_eq!(row["fileName"], "過去問.txt", "{row}");
    assert_eq!(row["fileSize"], 5, "{row}");
    assert_eq!(row["isText"], true, "{row}");
    assert_eq!(row["missing"], false, "{row}");
    assert!(row.get("path").is_none(), "閲覧の場にパスを出さない: {row}");

    let (status, body) = send_anon(app, "GET", &format!("/api/v1/contents/{id}/download")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "hello");
}

/// 開ける条件はコンテンツの公開範囲。ログインしていない人には「ログインした人」を開かせない。
#[sqlx::test]
async fn opening_a_shortcut_follows_its_visibility(pool: SqlitePool) {
    let dir = temp_test_dir("shortcut-visibility");
    let file = dir.join("a.txt");
    std::fs::write(&file, b"a").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;

    let id = create_shortcut(app.clone(), &cookie, &file, "authenticated").await;

    let uri = format!("/api/v1/contents/{id}/download");
    let (status, body) = send_anon(app.clone(), "GET", &uri).await;
    assert_error(status, &body, StatusCode::UNAUTHORIZED, "unauthorized");
    let (status, _) = send_empty(app, "GET", &uri, &cookie).await;
    assert_eq!(status, StatusCode::OK);
}

/// 種類と場所が合わないものは作れない。共有の外と、一覧に出さないファイルも選べない。
#[sqlx::test]
async fn a_shortcut_requires_a_listed_file_inside_a_share(pool: SqlitePool) {
    let dir = temp_test_dir("shortcut-validate");
    std::fs::write(dir.join("a.txt"), b"a").expect("ファイルを作れなかった");
    std::fs::write(dir.join(".dot"), b"a").expect("ファイルを作れなかった");
    let outside = std::env::temp_dir().join("weblav-shortcut-outside.txt");
    std::fs::write(&outside, b"a").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let create = |content_type: &str, path: &std::path::Path| {
        format!(
            r#"{{"type":"{content_type}","path":{}}}"#,
            json_string(&path.display().to_string())
        )
    };

    for body in [
        create("shortcut", &dir),
        create("shortcut", &dir.join(".dot")),
        create("shortcut", &outside),
        create("folder", &dir.join("a.txt")),
    ] {
        let (status, response) =
            send_json(app.clone(), "POST", "/api/v1/contents", &cookie, &body).await;
        assert_error(
            status,
            &response,
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request_body",
        );
    }
}

/// 先が無くなっても行は残り、開くと 404。題は直せて、場所を選び直せば開ける。
#[sqlx::test]
async fn a_shortcut_survives_a_missing_target_and_can_be_repointed(pool: SqlitePool) {
    let dir = temp_test_dir("shortcut-missing");
    let file = dir.join("a.txt");
    let next = dir.join("b.txt");
    std::fs::write(&file, b"a").expect("ファイルを作れなかった");
    std::fs::write(&next, b"b").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let id = create_shortcut(app.clone(), &cookie, &file, "public").await;
    let (_, body) = send_empty(app.clone(), "GET", "/api/v1/admin/contents", &cookie).await;
    let stored = parse_json(&body)
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["id"] == id).cloned())
        .expect("一覧に無かった")["path"]
        .as_str()
        .expect("パスがあるはず")
        .to_string();

    std::fs::remove_file(&file).expect("ファイルを消せなかった");

    let row = home_row(app.clone(), &cookie, id).await;
    assert_eq!(row["missing"], true, "{row}");
    let download = format!("/api/v1/contents/{id}/download");
    let (status, _) = send_anon(app.clone(), "GET", &download).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let put =
        |title: &str, path: &str| format!(r#"{{"title":"{title}","path":{}}}"#, json_string(path));
    let uri = format!("/api/v1/contents/{id}");
    let (status, body) = send_json(app.clone(), "PUT", &uri, &cookie, &put("題", &stored)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "題は、先が無くても直せるはず: {body}"
    );

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &uri,
        &cookie,
        &put("題", &next.display().to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse_json(&body)["type"], "shortcut", "{body}");
    let (status, body) = send_anon(app, "GET", &download).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "b");
}

/// 共有の画面の行の「ホームに載せる...」で作れる。行には掲載中の印が付く。
#[sqlx::test]
async fn a_share_file_can_be_listed_on_home(pool: SqlitePool) {
    let dir = temp_test_dir("shortcut-home");
    std::fs::write(dir.join("a.txt"), b"a").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let share = root_id_for(&pool, &dir).await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/v1/shares/{share}/list-on-home"),
        &cookie,
        &format!(
            r#"{{"path":{}}}"#,
            json_string(&rel_in_share(&pool, &dir, "a.txt").await)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let created = parse_json(&body);
    assert_eq!(created["type"], "shortcut", "{body}");
    assert_eq!(created["title"], "a.txt", "{body}");

    let parent = rel_in_share(&pool, &dir, "").await;
    let (status, body) = send_empty(
        app,
        "GET",
        &format!(
            "/api/v1/shares/{share}/browse?path={}",
            utf8_percent_encode(&parent, NON_ALPHANUMERIC)
        ),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let browse = parse_json(&body);
    let entry = browse["entries"]
        .as_array()
        .and_then(|entries| entries.iter().find(|entry| entry["name"] == "a.txt"))
        .expect("行があるはず");
    assert_eq!(entry["listedAs"]["contentId"], created["id"], "{body}");
}

/// 動かす・名前を変える・消す・上書きする前の確かめに、開けなくなるショートカットが入る。
/// 動かしても先は追わない (→ docs/shares.md「場所の付け替え」)。
#[sqlx::test]
async fn share_checks_name_affected_shortcuts_and_moves_do_not_follow(pool: SqlitePool) {
    let dir = temp_test_dir("shortcut-impact");
    std::fs::create_dir_all(dir.join("to")).expect("フォルダーを作れなかった");
    let file = dir.join("a.txt");
    std::fs::write(&file, b"a").expect("ファイルを作れなかった");
    std::fs::write(dir.join("to").join("a.txt"), b"other").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let share = root_id_for(&pool, &dir).await;
    let id = create_shortcut(app.clone(), &cookie, &file, "public").await;
    let title_of = |list: &serde_json::Value| {
        list.as_array().map(|rows| {
            rows.iter()
                .map(|row| row["contentId"].as_i64())
                .collect::<Vec<_>>()
        })
    };
    let rel = rel_in_share(&pool, &dir, "a.txt").await;
    let rel_to = rel_in_share(&pool, &dir, "to").await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/v1/shares/{share}/delete/check"),
        &cookie,
        &format!(r#"{{"paths":[{}]}}"#, json_string(&rel)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        title_of(&parse_json(&body)["listed"]),
        Some(vec![Some(id)]),
        "{body}"
    );

    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/v1/shares/{share}/rename/check"),
        &cookie,
        &format!(r#"{{"path":{}}}"#, json_string(&rel)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        title_of(&parse_json(&body)["unlinked"]),
        Some(vec![Some(id)]),
        "{body}"
    );

    let transfer = |op: &str, from: &str, to: &str| {
        format!(
            r#"{{"op":"{op}","from":{{"shareId":{share},"paths":[{}]}},"to":{{"shareId":{share},"path":{}}}}}"#,
            json_string(from),
            json_string(to)
        )
    };
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/shares/transfer/check",
        &cookie,
        &transfer("move", &rel, &rel_to),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let checked = parse_json(&body);
    assert_eq!(
        title_of(&checked["unlinked"]),
        Some(vec![Some(id)]),
        "{body}"
    );

    // コピーでは元は残るので、開けなくなるものは無い。置き先の同じ名前を上書きするものだけが入る。
    let target_id = create_shortcut(
        app.clone(),
        &cookie,
        &dir.join("to").join("a.txt"),
        "public",
    )
    .await;
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/shares/transfer/check",
        &cookie,
        &transfer("copy", &rel, &rel_to),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let checked = parse_json(&body);
    assert_eq!(title_of(&checked["unlinked"]), Some(vec![]), "{body}");
    assert_eq!(
        title_of(&checked["listed"]),
        Some(vec![Some(target_id)]),
        "{body}"
    );

    // 名前を変えても、先は追わない。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/v1/shares/{share}/rename"),
        &cookie,
        &format!(r#"{{"path":{},"name":"b.txt"}}"#, json_string(&rel)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let row = home_row(app, &cookie, id).await;
    assert_eq!(row["missing"], true, "{row}");
}
