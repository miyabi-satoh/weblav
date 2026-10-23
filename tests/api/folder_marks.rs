//! フォルダーの中の公開・非公開の印 (→ docs/folders.md「公開と非公開」)

use super::*;

fn marks_uri(id: i64) -> String {
    format!("/api/v1/contents/{id}/marks")
}

async fn set_mark(app: Router, cookie: &str, id: i64, path: &str, public: bool) {
    let (status, body) = send_json(
        app,
        "PUT",
        &marks_uri(id),
        cookie,
        &format!(r#"{{"path":{},"public":{public}}}"#, json_string(path)),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{path}: {body}");
}

/// 閲覧の一覧の、行の名前。
fn browse_names(body: &str) -> Vec<String> {
    parse_json(body)["entries"]
        .as_array()
        .expect("配列のはず")
        .iter()
        .filter_map(|entry| entry["name"].as_str().map(str::to_string))
        .collect()
}

/// 非公開にしたファイルとサブフォルダー (中ごと) は、一覧・ダウンロード・検索のどれにも出ない。
/// 書き換えられる人には、非公開の印を添えて並べる。印を取り消すとまた見られる。
#[sqlx::test]
async fn private_marks_are_left_out_until_cleared(pool: SqlitePool) {
    let dir = temp_test_dir("folder-hidden");
    std::fs::write(dir.join("keep.txt"), b"keep").expect("ファイルを作れなかった");
    std::fs::write(dir.join("secret.txt"), b"secret").expect("ファイルを作れなかった");
    std::fs::create_dir(dir.join("sub")).expect("フォルダーを作れなかった");
    std::fs::write(dir.join("sub").join("inner.txt"), b"inner").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let id = insert_folder_by(&pool, "教材", &*dir, "public", None, None).await;

    set_mark(app.clone(), &cookie, id, "secret.txt", false).await;
    set_mark(app.clone(), &cookie, id, "sub", false).await;

    let (status, body) =
        send_anon(app.clone(), "GET", &format!("/api/v1/contents/{id}/browse")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(browse_names(&body), ["keep.txt"], "{body}");

    let (status, body) = send(
        app.clone(),
        build_request(
            "GET",
            &format!("/api/v1/contents/{id}/browse"),
            Some(&cookie),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let private: Vec<String> = parse_json(&body)["entries"]
        .as_array()
        .expect("配列のはず")
        .iter()
        .filter(|entry| entry["public"] == false)
        .filter_map(|entry| entry["name"].as_str().map(str::to_string))
        .collect();
    assert_eq!(
        private,
        ["sub", "secret.txt"],
        "書き換えられる人には印を添えて並べる: {body}"
    );

    let download = |path: &str| format!("/api/v1/contents/{id}/download?path={path}");
    for path in ["secret.txt", "sub/inner.txt"] {
        let (status, _) = send_anon(app.clone(), "GET", &download(path)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
    let (status, _) = send_anon(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/browse?path=sub"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // 綴りの大文字小文字を変えても、実体のパスで突き合わせるので外れる (大文字小文字を区別しないディスクだけ)。
    if cfg!(target_os = "macos") {
        let (status, _) = send_anon(app.clone(), "GET", &download("SECRET.txt")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    let (status, body) = send_anon(app.clone(), "GET", "/api/v1/search?q=inner").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains("inner.txt"), "{body}");
    // 外していないものは当たる (検索がフォルダーの中を見ていることの確かめ)。
    let (status, body) = send_anon(app.clone(), "GET", "/api/v1/search?q=keep").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("keep.txt"), "{body}");

    let (status, body) = send(
        app.clone(),
        build_request("GET", &marks_uri(id), Some(&cookie), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let marks = parse_json(&body);
    let secret = marks
        .as_array()
        .expect("配列のはず")
        .iter()
        .find(|mark| mark["path"] == "secret.txt")
        .expect("secret.txt の印が無い")
        .clone();
    assert_eq!(marks.as_array().map(Vec::len), Some(2), "{body}");

    let (status, body) = send(
        app.clone(),
        build_request(
            "DELETE",
            &format!("{}/{}", marks_uri(id), secret["id"]),
            Some(&cookie),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _) = send_anon(app, "GET", &download("secret.txt")).await;
    assert_eq!(status, StatusCode::OK);
}

/// 新しく見つかったファイルを公開しないフォルダーでは、公開の印を付けたものだけが見え、
/// その上のフォルダーは辿れる。印の無い隣のファイルは見えない。
#[sqlx::test]
async fn folder_that_publishes_chosen_files_shows_only_marked_ones(pool: SqlitePool) {
    let dir = temp_test_dir("folder-chosen");
    std::fs::create_dir_all(dir.join("2024")).expect("フォルダーを作れなかった");
    std::fs::write(dir.join("2024").join("a.txt"), b"a").expect("ファイルを作れなかった");
    std::fs::write(dir.join("2024").join("b.txt"), b"b").expect("ファイルを作れなかった");
    std::fs::create_dir_all(dir.join("2023")).expect("フォルダーを作れなかった");
    std::fs::write(dir.join("2023").join("c.txt"), b"c").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let id = insert_folder_by(&pool, "写真", &*dir, "public", None, None).await;
    sqlx::query!("UPDATE contents SET new_files_public = 0 WHERE id = ?", id)
        .execute(&pool)
        .await
        .expect("設定を変えられなかった");
    set_mark(app.clone(), &cookie, id, "2024/a.txt", true).await;

    let browse = |path: &str| format!("/api/v1/contents/{id}/browse?path={path}");
    let (status, body) = send_anon(app.clone(), "GET", &browse("")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(browse_names(&body), ["2024"], "{body}");
    let (status, body) = send_anon(app.clone(), "GET", &browse("2024")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(browse_names(&body), ["a.txt"], "{body}");
    let (status, _) = send_anon(app, "GET", &browse("2023")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// フォルダーを非公開にすると、中で前に公開したものの印も消え、中ごと非公開になる。
#[sqlx::test]
async fn marking_a_folder_private_clears_marks_inside_it(pool: SqlitePool) {
    let dir = temp_test_dir("folder-mark-inside");
    std::fs::create_dir_all(dir.join("sub")).expect("フォルダーを作れなかった");
    std::fs::write(dir.join("sub").join("a.txt"), b"a").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let id = insert_folder_by(&pool, "教材", &*dir, "public", None, None).await;
    sqlx::query!("UPDATE contents SET new_files_public = 0 WHERE id = ?", id)
        .execute(&pool)
        .await
        .expect("設定を変えられなかった");
    set_mark(app.clone(), &cookie, id, "sub/a.txt", true).await;
    set_mark(app.clone(), &cookie, id, "sub", true).await;
    set_mark(app.clone(), &cookie, id, "sub", false).await;

    let (status, _) = send_anon(
        app,
        "GET",
        &format!("/api/v1/contents/{id}/download?path=sub/a.txt"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let paths: Vec<String> =
        sqlx::query_scalar!("SELECT path FROM folder_marks WHERE content_id = ?", id)
            .fetch_all(&pool)
            .await
            .expect("印を読めなかった");
    assert!(paths.is_empty(), "既定と同じになる印は残さない: {paths:?}");
}

/// 登録先を変えると、印は捨てる。登録先からの相対パスが、別のものを指すため。
#[sqlx::test]
async fn changing_the_folder_path_forgets_marks(pool: SqlitePool) {
    let first = temp_test_dir("folder-hidden-first");
    let second = temp_test_dir("folder-hidden-second");
    for dir in [&first, &second] {
        std::fs::write(dir.join("a.txt"), b"a").expect("ファイルを作れなかった");
    }
    let (app, cookie) = admin_app(&pool).await;
    let id = insert_folder_by(&pool, "教材", &*first, "public", None, None).await;
    set_mark(app.clone(), &cookie, id, "a.txt", false).await;

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}"),
        &cookie,
        &format!(
            r#"{{"title":"教材","path":{},"visibility":"public"}}"#,
            json_string(&second.display().to_string())
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, _) = send_anon(
        app,
        "GET",
        &format!("/api/v1/contents/{id}/download?path=a.txt"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// 登録先そのものと、フォルダーの外には印を付けられない。
#[sqlx::test]
async fn only_entries_inside_the_folder_can_be_marked(pool: SqlitePool) {
    let dir = temp_test_dir("folder-hidden-outside");
    let (app, cookie) = admin_app(&pool).await;
    let id = insert_folder_by(&pool, "教材", &*dir, "public", None, None).await;
    for body in [
        r#"{"path":"","public":false}"#,
        r#"{"path":"../x","public":false}"#,
        r#"{"path":"missing.txt","public":false}"#,
    ] {
        let (status, response) = send_json(app.clone(), "PUT", &marks_uri(id), &cookie, body).await;
        assert!(status.is_client_error(), "{body}: {status} {response}");
    }
}
