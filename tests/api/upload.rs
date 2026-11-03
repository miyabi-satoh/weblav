//! ファイルアップロード型コンテンツ。新しく作る口と実体の差し替えはもう無く、
//! 前から登録してあるものを読む・配る・題などを直す・消すことだけを確かめる。

use super::*;

/// 登録してあるファイルをダウンロードすると、種類と中身がそのまま返る。
#[sqlx::test]
async fn registered_file_content_downloads(pool: SqlitePool) {
    let (app, blobs_dir) = test_app_with_blobs(pool.clone()).await;
    let cookie = admin_cookie(app.clone(), &pool).await;
    let id = insert_file_content(
        &pool,
        &blobs_dir,
        ("資料", "doc.txt", b"hello world"),
        None,
        "public",
        None,
    )
    .await;

    let response = send_raw(
        app,
        build_request(
            "GET",
            &format!("/api/v1/contents/{id}/download"),
            Some(&cookie),
            None,
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/plain")
    );
    let disposition = response
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .expect("Content-Disposition が無い")
        .to_string();
    assert!(disposition.starts_with("inline"), "{disposition}");
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("本文を読めない");
    assert_eq!(&bytes[..], b"hello world");
}

/// 閲覧用の一覧 (`ContentResponse`) の file のフィールドが camelCase で出ること。
/// enum に付けた `rename_all` はバリアント名にしか効かず、気づかず snake_case に戻りやすい。
#[sqlx::test]
async fn list_contents_returns_file_fields_in_camel_case(pool: SqlitePool) {
    let (app, blobs_dir) = test_app_with_blobs(pool.clone()).await;
    let cookie = admin_cookie(app.clone(), &pool).await;
    insert_file_content(
        &pool,
        &blobs_dir,
        ("資料", "doc.txt", b"hello world"),
        None,
        "public",
        None,
    )
    .await;

    let (status, body) = send_empty(app, "GET", "/api/v1/contents", &cookie).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#""fileName":"doc.txt""#), "{body}");
    assert!(body.contains(r#""fileSize":11"#), "{body}");
}

/// 同じ中身の2行が1つのblobを指すとき、片方を削除してももう片方が参照している間はblobが残り、
/// 両方削除すると初めてblobが消える。
#[sqlx::test]
async fn deleting_file_contents_gcs_a_shared_blob_only_when_unreferenced(pool: SqlitePool) {
    let (app, blobs_dir) = test_app_with_blobs(pool.clone()).await;
    let cookie = admin_cookie(app.clone(), &pool).await;
    let mut ids = Vec::new();
    for title in ["1本目", "2本目"] {
        ids.push(
            insert_file_content(
                &pool,
                &blobs_dir,
                (title, "same.bin", b"identical bytes"),
                None,
                "public",
                None,
            )
            .await,
        );
    }
    assert_eq!(count_committed_blobs(&blobs_dir), 1);

    let (status, _) = send_empty(
        app.clone(),
        "DELETE",
        &format!("/api/v1/contents/{}", ids[0]),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        count_committed_blobs(&blobs_dir),
        1,
        "2本目がまだ参照しているのでblobは残るはず"
    );

    let (status, _) = send_empty(
        app,
        "DELETE",
        &format!("/api/v1/contents/{}", ids[1]),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        count_committed_blobs(&blobs_dir),
        0,
        "参照が無くなったのでblobは削除されるはず"
    );
}

/// `PUT /contents/{id}/upload` は題などだけを直し、ファイルはそのまま残す。
#[sqlx::test]
async fn replace_content_upload_updates_metadata_and_keeps_the_file(pool: SqlitePool) {
    let (app, blobs_dir) = test_app_with_blobs(pool.clone()).await;
    let cookie = admin_cookie(app.clone(), &pool).await;
    let id = insert_file_content(
        &pool,
        &blobs_dir,
        ("資料", "keep.txt", b"keep this content"),
        None,
        "public",
        None,
    )
    .await;

    let (status, body) = send(
        app.clone(),
        multipart_request(
            "PUT",
            &format!("/api/v1/contents/{id}/upload"),
            &cookie,
            &[
                ("title", "資料(タイトルだけ更新)"),
                ("visibility", "authenticated"),
            ],
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""fileName":"keep.txt""#), "{body}");
    assert!(body.contains(r#""visibility":"authenticated""#), "{body}");
    assert_eq!(count_committed_blobs(&blobs_dir), 1);

    let response = send_raw(
        app,
        build_request(
            "GET",
            &format!("/api/v1/contents/{id}/download"),
            Some(&cookie),
            None,
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("本文を読めない");
    assert_eq!(&bytes[..], b"keep this content");
}

/// ファイルの差し替えはもうしないので、`file` を送ると422にし、何も変えない。
#[sqlx::test]
async fn replace_content_upload_with_a_file_is_rejected(pool: SqlitePool) {
    let (app, blobs_dir) = test_app_with_blobs(pool.clone()).await;
    let cookie = admin_cookie(app.clone(), &pool).await;
    let id = insert_file_content(
        &pool,
        &blobs_dir,
        ("資料", "old.txt", b"old content"),
        None,
        "public",
        None,
    )
    .await;

    let (status, body) = send(
        app,
        multipart_request(
            "PUT",
            &format!("/api/v1/contents/{id}/upload"),
            &cookie,
            &[("title", "資料(更新)")],
            Some(("file", "new.txt", b"brand new content")),
        ),
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
    let title: String = sqlx::query_scalar("SELECT title FROM contents WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("行を読めない");
    assert_eq!(title, "資料");
}

/// `description`が`MAX_TEXT_FIELD_BYTES`(64KiB)を超えると、`read_text_field`の上限チェック(422)で弾かれる。
#[sqlx::test]
async fn replace_content_upload_with_oversized_text_field_is_validation_error(pool: SqlitePool) {
    let (app, blobs_dir) = test_app_with_blobs(pool.clone()).await;
    let cookie = admin_cookie(app.clone(), &pool).await;
    let id = insert_file_content(
        &pool,
        &blobs_dir,
        ("資料", "doc.txt", b"hello"),
        None,
        "public",
        None,
    )
    .await;

    let oversized_description = "a".repeat(128 * 1024);
    let (status, body) = send(
        app,
        multipart_request(
            "PUT",
            &format!("/api/v1/contents/{id}/upload"),
            &cookie,
            &[("title", "資料"), ("description", &oversized_description)],
            None,
        ),
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
}

/// `POST /contents`にtype=fileを送ると422。file コンテンツはもう作らない (ファイルは共有の中に置く)。
#[sqlx::test]
async fn create_content_json_with_file_type_is_validation_error(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/contents",
        &cookie,
        r#"{"type":"file","title":"資料"}"#,
    )
    .await;

    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
}

/// 種類 `archive` はフォルダーにまとめたので、もう受けない (要求の形の誤りとして 422)。
#[sqlx::test]
async fn create_content_with_the_removed_archive_type_is_rejected(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/contents",
        &cookie,
        r#"{"type":"archive","title":"英検 過去問"}"#,
    )
    .await;

    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
}

/// visibility に 'public' を持つ行がDBに入れられること(CHECK制約を置いていない確認)。
/// 匿名閲覧の判定自体は後続で入るため、ここではスキーマが受け付けることだけを見る。
#[sqlx::test]
async fn contents_accepts_public_visibility_at_schema_level(pool: SqlitePool) {
    sqlx::query(
        "INSERT INTO contents (type, title, url, visibility)
         VALUES ('link', '公開リンク', 'https://example.com', 'public')",
    )
    .execute(&pool)
    .await
    .expect("public visibility should be storable");

    let visibility: String =
        sqlx::query_scalar("SELECT visibility FROM contents WHERE title = '公開リンク'")
            .fetch_one(&pool)
            .await
            .expect("inserted row should be readable");

    assert_eq!(visibility, "public");
}

/// 一覧の表示を使うフォルダーを消すと、配下のアイテム・軸・軸の値・印がすべて連鎖削除されること。
#[sqlx::test]
async fn deleting_archive_cascades_to_items_axes_and_values(pool: SqlitePool) {
    sqlx::query(
        "INSERT INTO contents (id, type, title, path, visibility)
         VALUES (1, 'folder', '英検 過去問', '/tmp/eiken', 'public')",
    )
    .execute(&pool)
    .await
    .expect("archive row should be storable");

    sqlx::query(
        "INSERT INTO archive_items (archive_id, rel_path)
         VALUES (1, '2024/1/grade_2/listening.mp3')",
    )
    .execute(&pool)
    .await
    .expect("item should be storable");

    sqlx::query(
        "INSERT INTO archive_axes (id, archive_id, name, source, dir_level, position)
         VALUES (10, 1, '級', 'dir_level', 3, 0)",
    )
    .execute(&pool)
    .await
    .expect("axis should be storable");

    sqlx::query(
        "INSERT INTO archive_axis_values (axis_id, raw_value, display_name, position)
         VALUES (10, 'grade_2', '2級', 0)",
    )
    .execute(&pool)
    .await
    .expect("axis value should be storable");

    sqlx::query("DELETE FROM contents WHERE id = 1")
        .execute(&pool)
        .await
        .expect("archive should be deletable");

    let items: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM archive_items")
        .fetch_one(&pool)
        .await
        .expect("count should succeed");
    let axes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM archive_axes")
        .fetch_one(&pool)
        .await
        .expect("count should succeed");
    let values: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM archive_axis_values")
        .fetch_one(&pool)
        .await
        .expect("count should succeed");

    assert_eq!((items, axes, values), (0, 0, 0));
}

/// 自己参照FKのCASCADE。親グループを消すと子孫まで連鎖削除される。
/// (アプリケーション層は削除前に子をルート直下へ昇格させるため、この連鎖は最終防御)
/// スキーマの制約だけを確かめるため、ヘルパーを通さず id を固定した生の SQL で入れる。
#[sqlx::test]
async fn contents_self_referencing_cascade_removes_descendants(pool: SqlitePool) {
    sqlx::query("INSERT INTO contents (id, type, title) VALUES (10, 'group', '親')")
        .execute(&pool)
        .await
        .expect("parent group should be storable");

    sqlx::query(
        "INSERT INTO contents (id, type, parent_id, title)
         VALUES (7, 'group', 10, '子')",
    )
    .execute(&pool)
    .await
    .expect("child group should be storable");

    sqlx::query(
        "INSERT INTO contents (id, type, parent_id, title, url)
         VALUES (3, 'link', 7, '孫', 'https://example.com')",
    )
    .execute(&pool)
    .await
    .expect("grandchild link should be storable");

    sqlx::query("DELETE FROM contents WHERE id = 10")
        .execute(&pool)
        .await
        .expect("parent should be deletable");

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contents")
        .fetch_one(&pool)
        .await
        .expect("count should succeed");

    assert_eq!(remaining, 0);
}

/// fileコンテンツをJSON専用の`PUT /contents/{id}`で更新しようとすると422。
#[sqlx::test]
async fn update_content_json_on_file_row_is_validation_error(pool: SqlitePool) {
    let (app, blobs_dir) = test_app_with_blobs(pool.clone()).await;
    let cookie = admin_cookie(app.clone(), &pool).await;
    let id = insert_file_content(
        &pool,
        &blobs_dir,
        ("資料", "doc.txt", b"hello"),
        None,
        "public",
        None,
    )
    .await;

    let (status, _) = send_json(
        app,
        "PUT",
        &format!("/api/v1/contents/{id}"),
        &cookie,
        r#"{"title":"更新","url":null,"path":null,"description":null}"#,
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

/// link/folder行を`PUT /contents/{id}/upload`(multipart)で更新しようとすると422
/// (fileコンテンツ専用のエンドポイントであるため)。
#[sqlx::test]
async fn replace_content_upload_on_non_file_row_is_validation_error(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/contents",
        &cookie,
        r#"{"type":"link","title":"社内Wiki","url":"https://example.com"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = extract_id(&body);

    let response = app
        .oneshot(multipart_request(
            "PUT",
            &format!("/api/v1/contents/{id}/upload"),
            &cookie,
            &[("title", "更新")],
            None,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}
