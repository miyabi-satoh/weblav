//! フォルダーで表示しないにしたもの (→ docs/folders.md「表示しないもの」)

use super::*;

fn hidden_uri(id: i64) -> String {
    format!("/api/v1/contents/{id}/hidden-entries")
}

/// 表示しないにしたファイルとサブフォルダー (中ごと) は、一覧・ダウンロード・検索のどれにも出ない。
/// 取り消すとまた見られる。
#[sqlx::test]
async fn hidden_entries_are_left_out_until_shown_again(pool: SqlitePool) {
    let dir = temp_test_dir("folder-hidden");
    std::fs::write(dir.join("keep.txt"), b"keep").expect("ファイルを作れなかった");
    std::fs::write(dir.join("secret.txt"), b"secret").expect("ファイルを作れなかった");
    std::fs::create_dir(dir.join("sub")).expect("フォルダーを作れなかった");
    std::fs::write(dir.join("sub").join("inner.txt"), b"inner").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let id = insert_folder_by(&pool, "教材", &*dir, "public", None, None).await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        &hidden_uri(id),
        &cookie,
        r#"{"path":"secret.txt"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let secret = parse_json(&body);
    assert_eq!(secret["isDir"], false, "{body}");
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &hidden_uri(id),
        &cookie,
        r#"{"path":"sub"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse_json(&body)["isDir"], true, "{body}");

    let (status, body) =
        send_anon(app.clone(), "GET", &format!("/api/v1/contents/{id}/browse")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let names: Vec<String> = parse_json(&body)["entries"]
        .as_array()
        .expect("配列のはず")
        .iter()
        .filter_map(|entry| entry["name"].as_str().map(str::to_string))
        .collect();
    assert_eq!(names, ["keep.txt"], "{body}");

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
        build_request("GET", &hidden_uri(id), Some(&cookie), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        parse_json(&body).as_array().map(Vec::len),
        Some(2),
        "{body}"
    );

    let (status, body) = send(
        app.clone(),
        build_request(
            "DELETE",
            &format!("{}/{}", hidden_uri(id), secret["id"]),
            Some(&cookie),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _) = send_anon(app, "GET", &download("secret.txt")).await;
    assert_eq!(status, StatusCode::OK);
}

/// 登録先を変えると、表示しないにしたものは捨てる。登録先からの相対パスが、別のものを指すため。
#[sqlx::test]
async fn changing_the_folder_path_forgets_hidden_entries(pool: SqlitePool) {
    let first = temp_test_dir("folder-hidden-first");
    let second = temp_test_dir("folder-hidden-second");
    for dir in [&first, &second] {
        std::fs::write(dir.join("a.txt"), b"a").expect("ファイルを作れなかった");
    }
    let (app, cookie) = admin_app(&pool).await;
    let id = insert_folder_by(&pool, "教材", &*first, "public", None, None).await;
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &hidden_uri(id),
        &cookie,
        r#"{"path":"a.txt"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

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

/// 登録先そのものと、フォルダーの外は表示しないにできない。
#[sqlx::test]
async fn only_entries_inside_the_folder_can_be_hidden(pool: SqlitePool) {
    let dir = temp_test_dir("folder-hidden-outside");
    let (app, cookie) = admin_app(&pool).await;
    let id = insert_folder_by(&pool, "教材", &*dir, "public", None, None).await;
    for body in [
        r#"{"path":""}"#,
        r#"{"path":"../x"}"#,
        r#"{"path":"missing.txt"}"#,
    ] {
        let (status, response) =
            send_json(app.clone(), "POST", &hidden_uri(id), &cookie, body).await;
        assert!(status.is_client_error(), "{body}: {status} {response}");
    }
}
