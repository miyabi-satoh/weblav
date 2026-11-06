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
    let (_, body) = send_empty(app.clone(), "GET", "/api/v1/contents/manage", &cookie).await;
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

fn transfer_body(op: &str, share: i64, from: &str, to: &str, on_conflict: Option<&str>) -> String {
    let mut value = serde_json::json!({
        "op": op,
        "from": {"shareId": share, "paths": [from]},
        "to": {"shareId": share, "path": to},
    });
    if let Some(on_conflict) = on_conflict {
        value["onConflict"] = serde_json::json!(on_conflict);
    }
    value.to_string()
}

/// 移動を終わるまで流す。
async fn run_move(app: Router, cookie: &str, share: i64, from: &str, to: &str, on_conflict: &str) {
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/shares/transfer",
        cookie,
        &transfer_body("move", share, from, to, Some(on_conflict)),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let job = wait_job(app, cookie, &body).await;
    assert_eq!(job["state"], "succeeded", "{job}");
}

async fn download(app: Router, id: i64) -> (StatusCode, String) {
    send_anon(app, "GET", &format!("/api/v1/contents/{id}/download")).await
}

fn listed_ids(list: &serde_json::Value) -> Vec<i64> {
    list.as_array()
        .expect("配列のはず")
        .iter()
        .map(|row| row["contentId"].as_i64().expect("id のはず"))
        .collect()
}

/// 消す前と上書きする前の確かめに、響くショートカットが入る。
#[sqlx::test]
async fn delete_and_overwrite_checks_name_affected_shortcuts(pool: SqlitePool) {
    let dir = temp_test_dir("shortcut-impact");
    std::fs::create_dir_all(dir.join("to")).expect("フォルダーを作れなかった");
    std::fs::write(dir.join("a.txt"), b"a").expect("ファイルを作れなかった");
    std::fs::write(dir.join("to").join("a.txt"), b"other").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let share = root_id_for(&pool, &dir).await;
    let id = create_shortcut(app.clone(), &cookie, &dir.join("a.txt"), "public").await;
    let target_id = create_shortcut(
        app.clone(),
        &cookie,
        &dir.join("to").join("a.txt"),
        "public",
    )
    .await;
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
    assert_eq!(listed_ids(&parse_json(&body)["listed"]), vec![id], "{body}");

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/shares/transfer/check",
        &cookie,
        &transfer_body("copy", share, &rel, &rel_to, None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        listed_ids(&parse_json(&body)["listed"]),
        vec![target_id],
        "{body}"
    );
}

/// 共有の画面で名前を変えても動かしても、ショートカットは開ける (→ docs/shares.md「場所の付け替え」)。
#[sqlx::test]
async fn a_shortcut_still_opens_after_a_rename_and_a_move(pool: SqlitePool) {
    let dir = temp_test_dir("shortcut-follows");
    std::fs::create_dir_all(dir.join("to")).expect("フォルダーを作れなかった");
    std::fs::write(dir.join("a.txt"), b"a").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let share = root_id_for(&pool, &dir).await;
    let id = create_shortcut(app.clone(), &cookie, &dir.join("a.txt"), "public").await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/v1/shares/{share}/rename"),
        &cookie,
        &format!(
            r#"{{"path":{},"name":"b.txt"}}"#,
            json_string(&rel_in_share(&pool, &dir, "a.txt").await)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(download(app.clone(), id).await.1, "a");
    let row = home_row(app.clone(), &cookie, id).await;
    assert_eq!(row["fileName"], "b.txt", "{row}");
    assert_eq!(row["missing"], false, "{row}");

    run_move(
        app.clone(),
        &cookie,
        share,
        &rel_in_share(&pool, &dir, "b.txt").await,
        &rel_in_share(&pool, &dir, "to").await,
        "rename",
    )
    .await;
    let (status, body) = download(app, id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "a");
}

/// 別のショートカットの先に上書きで動かすと、どちらも動かしたファイルを開く。
#[sqlx::test]
async fn moving_over_another_shortcut_target_opens_the_moved_file_from_both(pool: SqlitePool) {
    let dir = temp_test_dir("shortcut-overwrite");
    std::fs::create_dir_all(dir.join("to")).expect("フォルダーを作れなかった");
    std::fs::write(dir.join("a.txt"), b"moved").expect("ファイルを作れなかった");
    std::fs::write(dir.join("to").join("a.txt"), b"old").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let share = root_id_for(&pool, &dir).await;
    let moved = create_shortcut(app.clone(), &cookie, &dir.join("a.txt"), "public").await;
    let target = create_shortcut(
        app.clone(),
        &cookie,
        &dir.join("to").join("a.txt"),
        "public",
    )
    .await;

    run_move(
        app.clone(),
        &cookie,
        share,
        &rel_in_share(&pool, &dir, "a.txt").await,
        &rel_in_share(&pool, &dir, "to").await,
        "overwrite",
    )
    .await;

    assert_eq!(download(app.clone(), moved).await.1, "moved");
    assert_eq!(download(app, target).await.1, "moved");
}

/// 先がフォルダーに置き換わったら、行を残して開けなくなる。
#[sqlx::test]
async fn a_shortcut_target_replaced_by_a_folder_shows_as_missing(pool: SqlitePool) {
    let dir = temp_test_dir("shortcut-replaced");
    std::fs::create_dir_all(dir.join("from").join("a")).expect("フォルダーを作れなかった");
    std::fs::write(dir.join("a"), b"old").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let share = root_id_for(&pool, &dir).await;
    let id = create_shortcut(app.clone(), &cookie, &dir.join("a"), "public").await;

    run_move(
        app.clone(),
        &cookie,
        share,
        &rel_in_share(&pool, &dir.join("from"), "a").await,
        &rel_in_share(&pool, &dir, "").await,
        "overwrite",
    )
    .await;

    assert_eq!(download(app.clone(), id).await.0, StatusCode::NOT_FOUND);
    let row = home_row(app, &cookie, id).await;
    assert_eq!(row["missing"], true, "{row}");
}

/// 先がどの共有の中でもなければ、ファイルがあっても開かない。付け替えた先も配信の判定を通る。
#[sqlx::test]
async fn a_shortcut_pointing_outside_every_share_does_not_open(pool: SqlitePool) {
    let dir = temp_test_dir("shortcut-outside");
    std::fs::write(dir.join("a.txt"), b"a").expect("ファイルを作れなかった");
    // `temp_test_dir` は共有の中に作るので、共有の外には直接作る。
    let outside_dir = test_support::TempDir::at(test_app_run_dir().join(format!(
        "shortcut-outside-{:?}",
        std::thread::current().id()
    )));
    let outside = outside_dir.join("outside.txt");
    std::fs::write(&outside, b"secret").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let id = create_shortcut(app.clone(), &cookie, &dir.join("a.txt"), "public").await;

    sqlx::query("UPDATE contents SET path = ? WHERE id = ?")
        .bind(outside.display().to_string())
        .bind(id)
        .execute(&pool)
        .await
        .expect("先を書き換えられなかった");

    assert_eq!(download(app.clone(), id).await.0, StatusCode::NOT_FOUND);
    let row = home_row(app, &cookie, id).await;
    assert_eq!(row["missing"], true, "{row}");
}
