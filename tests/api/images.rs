//! 画像のプレビュー (→ docs/ui.md「画像のプレビュー」)

use super::*;

/// テスト用の PNG。`image` crate で書く。
fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbImage::new(width, height)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("PNGを書けなかった");
    bytes
}

async fn get_with_headers(
    app: Router,
    uri: &str,
    headers: &[(header::HeaderName, &str)],
) -> Response {
    let mut request = Request::get(uri);
    for (name, value) in headers {
        request = request.header(name, *value);
    }
    send_raw(
        app,
        request
            .body(Body::empty())
            .expect("リクエストを組み立てられなかった"),
    )
    .await
}

/// フォルダの一覧は画像に大きさを添え、縮小画像は ETag で問い合わせられる。
/// 画像でないファイルには大きさを付けず、縮小画像も 404 にする。
#[sqlx::test]
async fn folder_images_get_sizes_and_thumbnails(pool: SqlitePool) {
    let dir = temp_test_dir("thumbnail-folder");
    std::fs::write(dir.join("photo.png"), png_bytes(300, 200)).expect("画像を作れなかった");
    std::fs::write(dir.join("note.txt"), b"hello").expect("ファイルを作れなかった");
    std::fs::write(dir.join("data.bin"), b"\x00\x01").expect("ファイルを作れなかった");
    std::fs::write(dir.join("print.pdf"), b"%PDF-1.4").expect("ファイルを作れなかった");
    let content_id = insert_folder_by(
        &pool,
        "写真",
        dir.display().to_string(),
        "public",
        None,
        None,
    )
    .await;
    let app = test_app(pool).await;

    let (status, body) = send_anon(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{content_id}/browse"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    let entry = |name: &str| {
        parsed["entries"]
            .as_array()
            .expect("entriesが配列でなかった")
            .iter()
            .find(|entry| entry["name"] == name)
            .cloned()
            .expect("エントリが無かった")
    };
    assert_eq!(
        entry("photo.png")["image"],
        serde_json::json!({"width": 300, "height": 200})
    );
    assert!(entry("note.txt")["image"].is_null(), "{body}");
    // テキストのビューアへの振り分けも同じ一覧に載る (→ docs/ui.md「PDF・動画・テキストのビューア」)。
    assert_eq!(entry("note.txt")["isText"], true, "{body}");
    assert_eq!(entry("data.bin")["isText"], false, "{body}");
    assert_eq!(entry("photo.png")["isText"], false, "{body}");
    // 縮小画像は、画像と、OS が作れる種類のファイルの行に出す (→ docs/ui.md「画像のプレビュー」)。
    assert_eq!(entry("photo.png")["thumbnail"], true, "{body}");
    assert_eq!(entry("note.txt")["thumbnail"], false, "{body}");
    assert_eq!(
        entry("print.pdf")["thumbnail"],
        weblav::os_thumbnail::SUPPORTED,
        "{body}"
    );

    let uri = format!("/api/v1/contents/{content_id}/thumbnail?path=photo.png");
    let response = get_with_headers(app.clone(), &uri, &[]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "image/jpeg");
    let etag = response.headers()[header::ETAG]
        .to_str()
        .expect("ETagが文字列でなかった")
        .to_string();

    let response = get_with_headers(app.clone(), &uri, &[(header::IF_NONE_MATCH, &etag)]).await;
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);

    let response = get_with_headers(
        app,
        &format!("/api/v1/contents/{content_id}/thumbnail?path=note.txt"),
        &[],
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// 縮小画像も、ダウンロードと同じ公開範囲の判定を通る。
#[sqlx::test]
async fn folder_thumbnail_anonymous_on_authenticated_folder_is_unauthorized(pool: SqlitePool) {
    let dir = temp_test_dir("thumbnail-folder-401");
    std::fs::write(dir.join("photo.png"), png_bytes(10, 10)).expect("画像を作れなかった");
    let content_id = insert_folder_by(
        &pool,
        "写真",
        dir.display().to_string(),
        "authenticated",
        None,
        None,
    )
    .await;
    let app = test_app(pool).await;

    let response = get_with_headers(
        app,
        &format!("/api/v1/contents/{content_id}/thumbnail?path=photo.png"),
        &[],
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// アップロードした画像も、ホームの一覧に大きさが載り、縮小画像を返す。
#[sqlx::test]
async fn uploaded_image_gets_size_in_home_list_and_thumbnail(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let png = png_bytes(120, 80);
    let response = app
        .clone()
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "写真")],
            Some(("file", "photo.png", &png)),
        ))
        .await
        .expect("リクエストの実行に失敗した");
    assert_eq!(response.status(), StatusCode::CREATED);
    let id = extract_id(&body_string(response).await);

    let (status, body) = send_empty(app.clone(), "GET", "/api/v1/contents", &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    let file = parsed
        .as_array()
        .expect("一覧が配列でなかった")
        .iter()
        .find(|content| content["id"] == id)
        .expect("アップロードしたファイルが一覧に無かった");
    assert_eq!(
        file["image"],
        serde_json::json!({"width": 120, "height": 80})
    );

    let response = get_with_headers(
        app,
        &format!("/api/v1/contents/{id}/thumbnail"),
        &[(header::COOKIE, &cookie)],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
}

/// アップロードしたテキストは、ホームの一覧でテキストのビューアに振り分けられる。
#[sqlx::test]
async fn uploaded_text_file_is_marked_as_text_in_home_list(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let response = app
        .clone()
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "設定")],
            Some(("file", "config.yml", b"name: weblav\n")),
        ))
        .await
        .expect("リクエストの実行に失敗した");
    assert_eq!(response.status(), StatusCode::CREATED);
    let id = extract_id(&body_string(response).await);

    let (status, body) = send_empty(app, "GET", "/api/v1/contents", &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    let file = parsed
        .as_array()
        .expect("一覧が配列でなかった")
        .iter()
        .find(|content| content["id"] == id)
        .expect("アップロードしたファイルが一覧に無かった");
    assert_eq!(file["isText"], true, "{body}");
}

/// グループの中にアップロードした画像も、グループの一覧に大きさが載る。
#[sqlx::test]
async fn uploaded_image_gets_size_in_group_list(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let group_id = insert_group_under(&pool, "アルバム", "authenticated", None).await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let png = png_bytes(60, 90);
    let parent = group_id.to_string();
    let response = app
        .clone()
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "写真"), ("parentId", &parent)],
            Some(("file", "photo.png", &png)),
        ))
        .await
        .expect("リクエストの実行に失敗した");
    assert_eq!(response.status(), StatusCode::CREATED);
    let id = extract_id(&body_string(response).await);

    let (status, body) = send_empty(
        app,
        "GET",
        &format!("/api/v1/contents/{group_id}/group"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    let file = parsed["entries"]
        .as_array()
        .expect("entries が配列でなかった")
        .iter()
        .find(|content| content["id"] == id)
        .expect("アップロードしたファイルがグループの一覧に無かった");
    assert_eq!(
        file["image"],
        serde_json::json!({"width": 60, "height": 90})
    );
}

/// アーカイブのアイテムも、閲覧用の一覧に大きさが載り、縮小画像を返す。
/// 非公開のアイテムの縮小画像は、ダウンロードと同じく 404。
#[sqlx::test]
async fn archive_image_items_get_sizes_and_thumbnails(pool: SqlitePool) {
    let dir = temp_test_dir("thumbnail-archive");
    std::fs::write(dir.join("photo.png"), png_bytes(64, 48)).expect("画像を作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, Some("png")).await;
    let (status, body) = rescan(app.clone(), &cookie, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/items"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    let item_id = items[0]["id"].as_i64().expect("idが返らなかった");
    let thumbnail_uri = format!("/api/v1/contents/{id}/items/{item_id}/thumbnail");

    let response = get_with_headers(app.clone(), &thumbnail_uri, &[]).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    publish_all_items(&pool, id).await;
    let (status, body) = view_archive(app.clone(), "", id, &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
    assert_eq!(
        parsed["items"][0]["image"],
        serde_json::json!({"width": 64, "height": 48})
    );
    assert_eq!(parsed["items"][0]["isText"], false, "{body}");

    let response = get_with_headers(app, &thumbnail_uri, &[]).await;
    assert_eq!(response.status(), StatusCode::OK);
}
