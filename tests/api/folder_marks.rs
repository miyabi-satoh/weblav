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
    sqlx::query!(
        "UPDATE contents SET list_view = 1, new_files_public = 0 WHERE id = ?",
        id
    )
    .execute(&pool)
    .await
    .expect("設定を変えられなかった");

    let browse = |path: &str| format!("/api/v1/contents/{id}/browse?path={path}");
    // まだ何も公開していなくても、登録先そのものは開けて、空に見える。
    let (status, body) = send_anon(app.clone(), "GET", &browse("")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(browse_names(&body).is_empty(), "{body}");

    set_mark(app.clone(), &cookie, id, "2024/a.txt", true).await;
    let (status, body) = send_anon(app.clone(), "GET", &browse("")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(browse_names(&body), ["2024"], "{body}");
    let (status, body) = send_anon(app.clone(), "GET", &browse("2024")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(browse_names(&body), ["a.txt"], "{body}");
    let (status, _) = send_anon(app, "GET", &browse("2023")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// 絞り込みで外れるファイルは、見る人にも編集できる人にも出ず、印も付けられない。
/// 前に付けた印は効かなくなり、その上のフォルダーも辿れなくなる。
#[sqlx::test]
async fn files_outside_the_patterns_are_left_out_for_everyone(pool: SqlitePool) {
    let dir = temp_test_dir("folder-patterns");
    std::fs::write(dir.join("a.pdf"), b"a").expect("ファイルを作れなかった");
    std::fs::write(dir.join("b.txt"), b"b").expect("ファイルを作れなかった");
    std::fs::create_dir_all(dir.join("notes")).expect("フォルダーを作れなかった");
    std::fs::write(dir.join("notes").join("c.txt"), b"c").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let id = insert_folder_by(&pool, "資料", &*dir, "public", None, None).await;
    sqlx::query!(
        "UPDATE contents SET list_view = 1, new_files_public = 0 WHERE id = ?",
        id
    )
    .execute(&pool)
    .await
    .expect("設定を変えられなかった");
    set_mark(app.clone(), &cookie, id, "a.pdf", true).await;
    set_mark(app.clone(), &cookie, id, "notes/c.txt", true).await;

    let browse = format!("/api/v1/contents/{id}/browse?path=");
    let (_, body) = send_anon(app.clone(), "GET", &browse).await;
    assert_eq!(browse_names(&body), ["notes", "a.pdf"], "{body}");

    sqlx::query!(
        "UPDATE contents SET include_patterns = '*.pdf' WHERE id = ?",
        id
    )
    .execute(&pool)
    .await
    .expect("設定を変えられなかった");

    let (status, body) = send_anon(app.clone(), "GET", &browse).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(browse_names(&body), ["a.pdf"], "{body}");
    let (_, body) = send_empty(app.clone(), "GET", &browse, &cookie).await;
    assert_eq!(browse_names(&body), ["notes", "a.pdf"], "{body}");

    let (status, _) = send_anon(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/download?path=notes/c.txt"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send_json(
        app.clone(),
        "PUT",
        &marks_uri(id),
        &cookie,
        r#"{"path":"b.txt","public":true}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, body) = send_empty(app.clone(), "GET", &marks_uri(id), &cookie).await;
    assert!(!body.contains("c.txt"), "{body}");
    let (_, body) = send_anon(app, "GET", "/api/v1/search?q=c.txt").await;
    assert!(!body.contains("c.txt"), "{body}");
}

/// 除外したフォルダーは、見る人にも編集できる人にも出ない。正しくないパターンは保存しない。
#[sqlx::test]
async fn excluded_folder_is_left_out_and_invalid_patterns_are_rejected(pool: SqlitePool) {
    let dir = temp_test_dir("folder-exclude");
    std::fs::create_dir_all(dir.join("drafts")).expect("フォルダーを作れなかった");
    std::fs::write(dir.join("drafts").join("a.txt"), b"a").expect("ファイルを作れなかった");
    std::fs::write(dir.join("b.txt"), b"b").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let id = insert_folder_by(&pool, "資料", &*dir, "public", None, None).await;
    let path_json = serde_json::to_string(&dir.display().to_string()).expect("JSON にできるはず");
    let put = |patterns: &'static str| {
        format!(r#"{{"title":"資料","path":{path_json},"excludePatterns":"{patterns}"}}"#)
    };

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}"),
        &cookie,
        &put("[a"),
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}"),
        &cookie,
        &put("drafts/"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let browse = format!("/api/v1/contents/{id}/browse?path=");
    let (_, body) = send_empty(app.clone(), "GET", &browse, &cookie).await;
    assert_eq!(browse_names(&body), ["b.txt"], "{body}");
    let (status, _) = send_anon(
        app,
        "GET",
        &format!("/api/v1/contents/{id}/browse?path=drafts"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// ライブラリをやめると、印の無いファイルは見えるようになる。保存の前に増える数を数えられる。
#[sqlx::test]
async fn leaving_library_shows_unmarked_files_and_can_be_previewed(pool: SqlitePool) {
    let dir = temp_test_dir("folder-preview");
    std::fs::write(dir.join("a.pdf"), b"a").expect("ファイルを作れなかった");
    std::fs::write(dir.join("b.txt"), b"b").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let id = insert_folder_by(&pool, "資料", &*dir, "public", None, None).await;
    sqlx::query!(
        "UPDATE contents SET list_view = 1, new_files_public = 0 WHERE id = ?",
        id
    )
    .execute(&pool)
    .await
    .expect("設定を変えられなかった");

    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("{}/preview", marks_uri(id)),
        &cookie,
        r#"{"listView":false,"newFilesPublic":false,"includePatterns":"*.pdf"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let preview: serde_json::Value = serde_json::from_str(&body).expect("JSON のはず");
    assert_eq!(preview["gained"], 1, "{body}");
    assert_eq!(preview["lost"], 0, "{body}");
    assert_eq!(preview["truncated"], false, "{body}");

    sqlx::query!("UPDATE contents SET list_view = 0 WHERE id = ?", id)
        .execute(&pool)
        .await
        .expect("設定を変えられなかった");
    let (_, body) = send_anon(app, "GET", &format!("/api/v1/contents/{id}/browse?path=")).await;
    assert_eq!(browse_names(&body), ["a.pdf", "b.txt"], "{body}");
}

/// 「新しく見つかったファイルを公開する」は、スキャンで初めて見つけたときの初期値だけを決める。
/// 設定を変えても、見つけ済みのファイルと手で変えたファイルの公開は変わらない。
#[sqlx::test]
async fn new_files_setting_applies_only_when_a_scan_first_finds_a_file(pool: SqlitePool) {
    let dir = temp_test_dir("folder-initial-public");
    std::fs::write(dir.join("hidden.txt"), b"h").expect("ファイルを作れなかった");
    std::fs::write(dir.join("chosen.txt"), b"c").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let id = insert_folder_by(&pool, "写真", &*dir, "public", None, None).await;
    sqlx::query!(
        "UPDATE contents SET list_view = 1, new_files_public = 0 WHERE id = ?",
        id
    )
    .execute(&pool)
    .await
    .expect("設定を変えられなかった");
    let rescan = format!("/api/v1/contents/{id}/rescan");
    let (status, body) = send_empty(app.clone(), "POST", &rescan, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    set_mark(app.clone(), &cookie, id, "chosen.txt", true).await;

    sqlx::query!("UPDATE contents SET new_files_public = 1 WHERE id = ?", id)
        .execute(&pool)
        .await
        .expect("設定を変えられなかった");
    let browse = format!("/api/v1/contents/{id}/browse?path=");
    let (_, body) = send_anon(app.clone(), "GET", &browse).await;
    assert_eq!(browse_names(&body), ["chosen.txt"], "{body}");

    // 手で非公開にしたものも、設定を変えて戻さない。
    set_mark(app.clone(), &cookie, id, "chosen.txt", false).await;
    sqlx::query!("UPDATE contents SET new_files_public = 0 WHERE id = ?", id)
        .execute(&pool)
        .await
        .expect("設定を変えられなかった");
    sqlx::query!("UPDATE contents SET new_files_public = 1 WHERE id = ?", id)
        .execute(&pool)
        .await
        .expect("設定を変えられなかった");

    // 新しいファイルは、見つけた時点の設定で入る。
    std::fs::write(dir.join("new.txt"), b"n").expect("ファイルを作れなかった");
    let (status, body) = send_empty(app.clone(), "POST", &rescan, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, body) = send_anon(app, "GET", &browse).await;
    assert_eq!(browse_names(&body), ["new.txt"], "{body}");
}

/// フォルダーを非公開にすると、中で前に公開したものの印も消え、中ごと非公開になる。
#[sqlx::test]
async fn marking_a_folder_private_clears_marks_inside_it(pool: SqlitePool) {
    let dir = temp_test_dir("folder-mark-inside");
    std::fs::create_dir_all(dir.join("sub")).expect("フォルダーを作れなかった");
    std::fs::write(dir.join("sub").join("a.txt"), b"a").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let id = insert_folder_by(&pool, "教材", &*dir, "public", None, None).await;
    sqlx::query!(
        "UPDATE contents SET list_view = 1, new_files_public = 0 WHERE id = ?",
        id
    )
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
