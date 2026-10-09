//! 作成の入力は必要最小限 (→ docs/ui.md「UI 全般」)

use super::*;

/// フォルダーはタイトルを省くと、選んだフォルダーの名前になる。同じ親に同じ名前があれば連番を付ける。
#[sqlx::test]
async fn a_folder_without_a_title_is_named_after_the_folder_and_numbered(pool: SqlitePool) {
    let dir = temp_test_dir("auto-title-folder");
    let folder_name = dir
        .file_name()
        .expect("フォルダー名がある")
        .to_string_lossy()
        .into_owned();
    let (app, cookie) = admin_app(&pool).await;
    let body = format!(
        r#"{{"type":"folder","path":{}}}"#,
        json_string(&dir.display().to_string())
    );

    let (status, first) = send_json(app.clone(), "POST", "/api/v1/contents", &cookie, &body).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    assert!(
        first.contains(&format!(r#""title":{}"#, json_string(&folder_name))),
        "{first}"
    );

    let (status, second) = send_json(app, "POST", "/api/v1/contents", &cookie, &body).await;
    assert_eq!(status, StatusCode::CREATED, "{second}");
    assert!(
        second.contains(&format!(
            r#""title":{}"#,
            json_string(&format!("{folder_name} (1)"))
        )),
        "{second}"
    );
}

/// ファイルはタイトルを省くと、拡張子を除いたファイル名になる。
/// 連番の付け方は `resolve_create_title` の単体テストで見る。
#[sqlx::test]
async fn an_upload_without_a_title_is_named_after_the_file(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send(
        app,
        multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[],
            Some(("file", "問題.pdf", b"x")),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(body.contains(r#""title":"問題""#), "{body}");
}

/// 公開できるフォルダーは、名前を省くとフォルダー名になり、重なれば連番を付ける。
#[sqlx::test]
async fn a_root_without_a_name_gets_a_numbered_folder_name(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    let first = root_dir("roots-auto-name-1").join("same");
    let second = root_dir("roots-auto-name-2").join("same");
    std::fs::create_dir_all(&first).expect("フォルダーを作れなかった");
    std::fs::create_dir_all(&second).expect("フォルダーを作れなかった");

    for (dir, expected) in [(&first, "same"), (&second, "same (1)")] {
        let path = canonical_path_as_api_returns_it(
            &std::fs::canonicalize(dir).expect("canonicalize できなかった"),
        );
        let (status, body) = send(
            app.clone(),
            roots_request("POST", ROOTS_URI, &cookie, Some(&json_path(&path))),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert!(body.contains(&format!(r#""name":"{expected}""#)), "{body}");
    }
}

/// ユーザーは権限を省くと `user` で作られる。
#[sqlx::test]
async fn a_user_created_without_a_role_is_a_regular_user(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/admin/users",
        &cookie,
        r#"{"username":"carol","password":"pw"}"#,
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(body.contains(r#""role":"user""#), "{body}");
}
