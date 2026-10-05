//! ファイルアップロード型コンテンツ

use super::*;

/// アップロード成功(201)・レスポンス内容・ダウンロード(Content-Type/本文)を確認する。
#[sqlx::test]
async fn upload_content_creates_and_downloads_file(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let response = app
        .clone()
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "資料"), ("description", "説明")],
            Some(("file", "doc.txt", b"hello world")),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let body = body_string(response).await;
    assert!(body.contains(r#""type":"file""#), "{body}");
    assert!(body.contains(r#""title":"資料""#), "{body}");
    assert!(body.contains(r#""fileName":"doc.txt""#), "{body}");
    assert!(body.contains(r#""fileSize":11"#), "{body}");
    let id = extract_id(&body);

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
        .unwrap()
        .to_string();
    assert!(disposition.starts_with("inline"), "{disposition}");
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&bytes[..], b"hello world");
}

/// 閲覧用の一覧 (`ContentResponse`) の file のフィールドが camelCase で出ること。
/// enum に付けた `rename_all` はバリアント名にしか効かず、気づかず snake_case に戻りやすい。
#[sqlx::test]
async fn list_contents_returns_file_fields_in_camel_case(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let response = app
        .clone()
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "資料"), ("visibility", "public")],
            Some(("file", "doc.txt", b"hello world")),
        ))
        .await
        .expect("アップロードのリクエストを送れなかった");
    assert_eq!(response.status(), StatusCode::CREATED);

    let (status, body) = send_empty(app, "GET", "/api/v1/contents", &cookie).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#""fileName":"doc.txt""#), "{body}");
    assert!(body.contains(r#""fileSize":11"#), "{body}");
}

/// 同一内容を2行にアップロードするとblobは1つに重複排除され、片方を削除しても
/// もう片方が参照している間はblobが残り、両方削除すると初めてblobが消える。
#[sqlx::test]
async fn upload_content_dedupes_blob_and_gcs_on_delete(pool: SqlitePool) {
    let (app, blobs_dir) = test_app_with_blobs(pool.clone()).await;
    let cookie = admin_cookie(app.clone(), &pool).await;

    let upload = |app: Router, cookie: String, title: &'static str| {
        let cookie = cookie.clone();
        async move {
            let response = app
                .oneshot(multipart_request(
                    "POST",
                    "/api/v1/contents/upload",
                    &cookie,
                    &[("title", title)],
                    Some(("file", "same.bin", b"identical bytes")),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::CREATED);
            let body = body_string(response).await;
            extract_id(&body)
        }
    };

    let id1 = upload(app.clone(), cookie.clone(), "1本目").await;
    let id2 = upload(app.clone(), cookie.clone(), "2本目").await;
    assert_eq!(
        count_committed_blobs(&blobs_dir),
        1,
        "同一内容は重複排除されるはず"
    );

    let (status, _) = send_empty(
        app.clone(),
        "DELETE",
        &format!("/api/v1/contents/{id1}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        count_committed_blobs(&blobs_dir),
        1,
        "id2がまだ参照しているのでblobは残るはず"
    );

    let (status, _) = send_empty(app, "DELETE", &format!("/api/v1/contents/{id2}"), &cookie).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        count_committed_blobs(&blobs_dir),
        0,
        "参照が無くなったのでblobは削除されるはず"
    );
}

/// TOCTOU回帰テスト: 「id1(最後の参照)を削除してGCする」リクエストと「id1と同一内容を
/// 新規アップロードする(id2)」リクエストを`tokio::join!`で同時に実行する。
/// `AppState::contents_write_lock`が無いと、削除側のGC(参照数COUNT→削除)と新規アップロード側の
/// commit_blob→INSERTが競合し、「id2の行はあるのに実体(blob)が消えている」状態になり得る
/// (delete_content/gc_blob_if_unreferenced・upload_content参照)。`contents_write_lock`で
/// commit_blob〜INSERT〜GCが直列化されている限り、どちらの順で実行されてもid2の実体は
/// 必ず存在するはず。
///
/// 注意: `tokio::join!`はスケジューラの都合で必ずしも「危険な順序」を再現するとは限らない
/// (完全に決定的な再現には`tokio::time::pause`等で手動にステップ制御する必要がある)。
/// それでも、この回帰が再発すればいずれかの実行順序で本アサーションが偶発的に失敗する
/// ようになるため、放置よりは有用な安全網とする。
#[sqlx::test]
async fn concurrent_delete_and_upload_of_same_blob_does_not_orphan_row(pool: SqlitePool) {
    let (app, blobs_dir) = test_app_with_blobs(pool.clone()).await;
    let cookie = admin_cookie(app.clone(), &pool).await;

    let response = app
        .clone()
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "並行1本目")],
            Some(("file", "race.bin", b"race condition payload")),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = body_string(response).await;
    let id1 = extract_id(&body);

    let delete_fut = {
        let app = app.clone();
        let cookie = cookie.clone();
        async move {
            send_raw(
                app,
                build_request(
                    "DELETE",
                    &format!("/api/v1/contents/{id1}"),
                    Some(&cookie),
                    None,
                ),
            )
            .await
        }
    };
    let upload_fut = {
        let app = app.clone();
        let cookie = cookie.clone();
        async move {
            app.oneshot(multipart_request(
                "POST",
                "/api/v1/contents/upload",
                &cookie,
                &[("title", "並行2本目")],
                Some(("file", "race.bin", b"race condition payload")),
            ))
            .await
            .unwrap()
        }
    };

    let (delete_response, upload_response) = tokio::join!(delete_fut, upload_fut);

    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);
    assert_eq!(upload_response.status(), StatusCode::CREATED);
    let body = body_string(upload_response).await;
    let id2 = extract_id(&body);

    let response = send_raw(
        app,
        build_request(
            "GET",
            &format!("/api/v1/contents/{id2}/download"),
            Some(&cookie),
            None,
        ),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "id2のblobが消えている(削除側GCとアップロード側commit_blobの競合が再発している可能性)"
    );
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&bytes[..], b"race condition payload");

    std::fs::remove_dir_all(&blobs_dir).ok();
}

/// `file`を指定した差し替えでは実体が置き換わり、旧blobが他行から参照されていなければ
/// 削除される。ダウンロードすると新しい内容が返る。
#[sqlx::test]
async fn replace_content_upload_replaces_blob_and_gcs_old_one(pool: SqlitePool) {
    let (app, blobs_dir) = test_app_with_blobs(pool.clone()).await;
    let cookie = admin_cookie(app.clone(), &pool).await;

    let response = app
        .clone()
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "資料")],
            Some(("file", "old.txt", b"old content")),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = body_string(response).await;
    let id = extract_id(&body);
    assert_eq!(count_committed_blobs(&blobs_dir), 1);

    let response = app
        .clone()
        .oneshot(multipart_request(
            "PUT",
            &format!("/api/v1/contents/{id}/upload"),
            &cookie,
            &[("title", "資料(更新)")],
            Some(("file", "new.txt", b"brand new content")),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_string(response).await;
    assert!(body.contains(r#""fileName":"new.txt""#), "{body}");
    // 旧blobがGCされ、新blob1つだけが残っているはず(旧新2つ残っていない)。
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
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&bytes[..], b"brand new content");
}

/// `file`フィールドを省略した差し替えは既存のblobを維持する(メタデータだけ更新)。
#[sqlx::test]
async fn replace_content_upload_without_file_keeps_existing_blob(pool: SqlitePool) {
    let (app, blobs_dir) = test_app_with_blobs(pool.clone()).await;
    let cookie = admin_cookie(app.clone(), &pool).await;

    let response = app
        .clone()
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "資料")],
            Some(("file", "keep.txt", b"keep this content")),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = body_string(response).await;
    let id = extract_id(&body);

    let response = app
        .clone()
        .oneshot(multipart_request(
            "PUT",
            &format!("/api/v1/contents/{id}/upload"),
            &cookie,
            &[("title", "資料(タイトルだけ更新)")],
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_string(response).await;
    assert!(body.contains(r#""fileName":"keep.txt""#), "{body}");
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
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&bytes[..], b"keep this content");
}

/// 同一内容での再アップロード(差し替え)は新旧のハッシュが一致するため、GC判定で
/// 誤って自分自身のblobを消してしまわないことを確認する(UPDATE確定後にCOUNTする
/// ロジックの回帰テスト)。
#[sqlx::test]
async fn replace_content_upload_with_identical_bytes_keeps_blob(pool: SqlitePool) {
    let (app, blobs_dir) = test_app_with_blobs(pool.clone()).await;
    let cookie = admin_cookie(app.clone(), &pool).await;

    let response = app
        .clone()
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "資料")],
            Some(("file", "same.txt", b"same content")),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = body_string(response).await;
    let id = extract_id(&body);

    let response = app
        .clone()
        .oneshot(multipart_request(
            "PUT",
            &format!("/api/v1/contents/{id}/upload"),
            &cookie,
            &[("title", "資料")],
            Some(("file", "same.txt", b"same content")),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
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
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&bytes[..], b"same content");
}

/// `file`フィールドが無いアップロードは422(共通envelope)。
#[sqlx::test]
async fn upload_content_without_file_is_validation_error(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let response = app
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "資料")],
            None,
        ))
        .await
        .unwrap();

    let status = response.status();
    let body = body_string(response).await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
}

/// `description`が`MAX_TEXT_FIELD_BYTES`(64KiB)を超えるが`DefaultBodyLimit`の
/// バックストップ(既定の`config.upload`では100MiB超)には遠く及ばない場合、
/// アプリ側の`read_text_field`の上限チェック(422)で弾かれることを確認する。
/// `upload_content_exceeding_body_limit_backstop_is_rejected`(413)とは別の経路。
#[sqlx::test]
async fn upload_content_with_oversized_text_field_is_validation_error(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let oversized_description = "a".repeat(128 * 1024);
    let response = app
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "資料"), ("description", &oversized_description)],
            Some(("file", "doc.txt", b"hello")),
        ))
        .await
        .unwrap();

    let status = response.status();
    let body = body_string(response).await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
}

/// 一般ユーザーもアップロードできて、公開範囲も選べる (→ docs/access.md「ロールと操作」)。
#[sqlx::test]
async fn upload_content_by_regular_user_keeps_requested_visibility(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let (app, _blobs_dir) = test_app_with_blobs(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let response = app
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "資料"), ("visibility", "authenticated")],
            Some(("file", "doc.txt", b"hello")),
        ))
        .await
        .expect("request should not fail");

    assert_eq!(response.status(), StatusCode::CREATED);
    let body = body_string(response).await;
    assert!(body.contains(r#""visibility":"authenticated""#), "{body}");
}

/// 設定されたサイズ上限を超えるアップロードは413(共通envelope、`file_too_large`)。
#[sqlx::test]
async fn upload_content_over_size_limit_is_rejected(pool: SqlitePool) {
    let data_dir = temp_test_dir("upload-oversize");
    let config = Config {
        upload: weblav::config::UploadConfig { max_size_mb: 1 },
        ..Config::default()
    };
    let app = app_in(pool.clone(), &config, &data_dir).await;
    let cookie = admin_cookie(app.clone(), &pool).await;

    let oversized = vec![0u8; 1024 * 1024 + 1];
    let response = app
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "資料")],
            Some(("file", "big.bin", &oversized)),
        ))
        .await
        .unwrap();

    let status = response.status();
    let body = body_string(response).await;
    assert_error(
        status,
        &body,
        StatusCode::PAYLOAD_TOO_LARGE,
        "file_too_large",
    );
}

/// アプリ側のストリーミングチェック(`file`フィールドにのみ効く)ではなく、
/// axum側`DefaultBodyLimit`のバックストップが効くケース。テキストフィールド
/// (`description`)自体をバックストップ超えの大きさにすることで再現する。
/// バックストップ超過も同じ`file_too_large`(413)の共通envelopeになることを確認する。
#[sqlx::test]
async fn upload_content_exceeding_body_limit_backstop_is_rejected(pool: SqlitePool) {
    let data_dir = temp_test_dir("upload-body-limit-backstop");
    let config = Config {
        upload: weblav::config::UploadConfig { max_size_mb: 1 },
        ..Config::default()
    };
    let app = app_in(pool.clone(), &config, &data_dir).await;
    let cookie = admin_cookie(app.clone(), &pool).await;

    // max_size_mb=1 のバックストップは max_bytes(1MiB) + UPLOAD_BODY_OVERHEAD_BYTES(1MiB)
    // = 2MiB。descriptionフィールド自体を3MiBにすればそれを超える。
    let huge_description = "a".repeat(3 * 1024 * 1024);
    let response = app
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("description", &huge_description)],
            None,
        ))
        .await
        .unwrap();

    let status = response.status();
    let body = body_string(response).await;
    assert_error(
        status,
        &body,
        StatusCode::PAYLOAD_TOO_LARGE,
        "file_too_large",
    );
}

/// JSON専用の`POST /contents`にtype=fileを送ると422(専用アップロードAPIへ誘導)。
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

/// type=archive はスキーマだけ先に用意してある段階なので、作成は422で断る。
/// スキャン・軸・アイテムが揃うまで、中身を持たないアーカイブ行を作らせない。
#[sqlx::test]
async fn create_content_with_archive_type_and_no_path_is_validation_error(pool: SqlitePool) {
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

/// アーカイブを消すと、配下のアイテム・軸・軸の値がすべて連鎖削除されること。
#[sqlx::test]
async fn deleting_archive_cascades_to_items_axes_and_values(pool: SqlitePool) {
    sqlx::query(
        "INSERT INTO contents (id, type, title, path, visibility)
         VALUES (1, 'archive', '英検 過去問', '/tmp/eiken', 'public')",
    )
    .execute(&pool)
    .await
    .expect("archive row should be storable");

    sqlx::query(
        "INSERT INTO archive_items (archive_id, rel_path, published)
         VALUES (1, '2024/1/grade_2/listening.mp3', 1)",
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
    let (app, cookie) = admin_app(&pool).await;

    let response = app
        .clone()
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "資料")],
            Some(("file", "doc.txt", b"hello")),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = body_string(response).await;
    let id = extract_id(&body);

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
