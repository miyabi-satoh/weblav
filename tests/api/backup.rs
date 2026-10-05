//! バックアップとリストア (→ docs/access.md「バックアップとリストア」)

use super::*;

const BACKUP_URI: &str = "/api/v1/admin/backup";
const BACKUP_STAGE_URI: &str = "/api/v1/admin/backup/restore/stage";
const BACKUP_RESTORE_URI: &str = "/api/v1/admin/backup/restore";
const SETUP_RESTORE_STAGE_URI: &str = "/api/v1/setup/restore/stage";
const SETUP_RESTORE_URI: &str = "/api/v1/setup/restore";

/// バックアップを落とし、zip の中身を返す。
async fn download_backup(app: Router, cookie: &str) -> Vec<u8> {
    let response = send_raw(app, build_request("GET", BACKUP_URI, Some(cookie), None)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/zip",
        "zip として返す"
    );
    to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("バックアップを読み取れなかった")
        .to_vec()
}

/// バックアップを受け取らせ、`(ステータス, 本文)` を返す。
async fn stage_backup(app: Router, cookie: &str, zip: &[u8]) -> (StatusCode, String) {
    send(
        app,
        multipart_request(
            "POST",
            BACKUP_STAGE_URI,
            cookie,
            &[],
            Some(("file", "backup.zip", zip)),
        ),
    )
    .await
}

fn staged_id(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body).expect("レスポンスをJSONとして読めなかった")
        ["id"]
        .as_str()
        .expect("idが返らなかった")
        .to_string()
}

/// 目録と DB だけの zip を作る。バックアップとして読めない形を試すのに使う。
fn handmade_backup(manifest: &str, db: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("manifest.json", options)
        .expect("目録を書けなかった");
    zip.write_all(manifest.as_bytes())
        .expect("目録を書けなかった");
    zip.start_file("weblav.db", options)
        .expect("DB を書けなかった");
    zip.write_all(db).expect("DB を書けなかった");
    zip.finish().expect("zip を閉じられなかった").into_inner()
}

async fn content_titles(pool: &SqlitePool) -> Vec<String> {
    sqlx::query_scalar("SELECT title FROM contents ORDER BY title")
        .fetch_all(pool)
        .await
        .expect("コンテンツを読めなかった")
}

/// 作ったバックアップで戻すと、作った時点の DB とアップロードしたファイルに戻る。
/// セッションは全部消えるので、戻した人もログインし直す。
#[sqlx::test]
async fn restoring_a_backup_brings_back_the_data_and_files(pool: SqlitePool) {
    let (app, blobs_dir) = test_app_with_blobs(pool.clone()).await;
    let cookie = admin_cookie(app.clone(), &pool).await;
    insert_user(&pool, "taro", "password").await;
    let other_cookie = login(app.clone(), "taro", "password").await;

    let response = app
        .clone()
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "資料")],
            Some(("file", "doc.txt", b"hello backup")),
        ))
        .await
        .expect("request should not fail");
    assert_eq!(response.status(), StatusCode::CREATED);
    let file_id = created_id(response).await;

    let zip = download_backup(app.clone(), &cookie).await;

    // バックアップの後の変更。ファイルを消すと、その実体も消える。
    let (status, _) = send_empty(
        app.clone(),
        "DELETE",
        &format!("/api/v1/contents/{file_id}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(count_committed_blobs(&blobs_dir), 0);
    insert_link_by(&pool, "後で足したリンク", "public", None, None).await;

    let (status, body) = stage_backup(app.clone(), &cookie, &zip).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains(&format!(r#""appVersion":"{}""#, env!("CARGO_PKG_VERSION"))),
        "{body}"
    );
    // 受け取っただけでは、まだ置き換えない。
    assert_eq!(content_titles(&pool).await, ["後で足したリンク"]);

    let (status, body) = send_json(
        app.clone(),
        "POST",
        BACKUP_RESTORE_URI,
        &cookie,
        &format!(r#"{{"id":"{}"}}"#, staged_id(&body)),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    assert_eq!(content_titles(&pool).await, ["資料"]);
    assert_eq!(count_committed_blobs(&blobs_dir), 1);
    for cookie in [&cookie, &other_cookie] {
        let (status, _) = send_empty(app.clone(), "GET", "/api/v1/auth/me", cookie).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "戻したらログインし直す");
    }

    let cookie = login(app.clone(), "admin-alice", "correct-password").await;
    let (status, body) = send_empty(
        app,
        "GET",
        &format!("/api/v1/contents/{file_id}/download"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "hello backup");
}

/// 戻した後で消えたユーザーの id を、新しいユーザーに使い回さない (AUTOINCREMENT の続きも戻す)。
#[sqlx::test]
async fn restoring_keeps_ids_of_deleted_users_retired(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    insert_user(&pool, "taro", "password").await;
    sqlx::query("DELETE FROM users WHERE username = 'taro'")
        .execute(&pool)
        .await
        .expect("ユーザーを消せなかった");
    let retired: i64 = sqlx::query_scalar("SELECT seq FROM sqlite_sequence WHERE name = 'users'")
        .fetch_one(&pool)
        .await
        .expect("続きの番号を読めなかった");

    let zip = download_backup(app.clone(), &cookie).await;
    let (_, body) = stage_backup(app.clone(), &cookie, &zip).await;
    sqlx::query("DELETE FROM sqlite_sequence WHERE name = 'users'")
        .execute(&pool)
        .await
        .expect("続きの番号を消せなかった");
    let (status, _) = send_json(
        app,
        "POST",
        BACKUP_RESTORE_URI,
        &cookie,
        &format!(r#"{{"id":"{}"}}"#, staged_id(&body)),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let restored: i64 = sqlx::query_scalar("SELECT seq FROM sqlite_sequence WHERE name = 'users'")
        .fetch_one(&pool)
        .await
        .expect("続きの番号を読めなかった");
    assert_eq!(restored, retired);
}

/// フォルダへの読む許可のブックマークはこの PC の状態なので、戻しても今のものを残す。
#[sqlx::test]
async fn restoring_keeps_the_folder_bookmarks_of_this_pc(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    let zip = download_backup(app.clone(), &cookie).await;
    let (_, body) = stage_backup(app.clone(), &cookie, &zip).await;
    sqlx::query("INSERT INTO folder_bookmarks (path, bookmark) VALUES ('/picked', x'00')")
        .execute(&pool)
        .await
        .expect("ブックマークを入れられなかった");

    let (status, _) = send_json(
        app,
        "POST",
        BACKUP_RESTORE_URI,
        &cookie,
        &format!(r#"{{"id":"{}"}}"#, staged_id(&body)),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let paths: Vec<String> = sqlx::query_scalar("SELECT path FROM folder_bookmarks")
        .fetch_all(&pool)
        .await
        .expect("ブックマークを読めなかった");
    assert_eq!(paths, ["/picked"]);
}

#[sqlx::test]
async fn staging_a_file_that_is_not_a_backup_is_rejected(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    for content in [
        b"not a zip".to_vec(),
        handmade_backup("{}", b""),
        handmade_backup(
            r#"{"format":1,"appVersion":"0.1.0","createdAt":0,"schemaVersion":0}"#,
            b"not a database",
        ),
    ] {
        let (status, body) = stage_backup(app.clone(), &cookie, &content).await;
        if status == StatusCode::OK {
            // 目録は読めても、DB が壊れていれば戻すときに断る。
            let (status, body) = send_json(
                app.clone(),
                "POST",
                BACKUP_RESTORE_URI,
                &cookie,
                &format!(r#"{{"id":"{}"}}"#, staged_id(&body)),
            )
            .await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
            assert!(body.contains(r#""kind":"backupInvalid""#), "{body}");
        } else {
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
            assert!(body.contains(r#""kind":"backupInvalid""#), "{body}");
        }
    }
    // 今のデータは変わらない。
    let (status, _) = send_empty(app, "GET", "/api/v1/auth/me", &cookie).await;
    assert_eq!(status, StatusCode::OK);
}

#[sqlx::test]
async fn staging_a_backup_from_a_newer_version_is_rejected(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let zip = handmade_backup(
        r#"{"format":1,"appVersion":"99.0.0","createdAt":0,"schemaVersion":99999999999999}"#,
        b"",
    );
    let (status, body) = stage_backup(app, &cookie, &zip).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body.contains(r#""kind":"backupTooNew","appVersion":"99.0.0""#),
        "{body}"
    );
}

#[sqlx::test]
async fn restoring_an_unknown_id_is_not_found(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, _) = send_json(app, "POST", BACKUP_RESTORE_URI, &cookie, r#"{"id":"x"}"#).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

fn setup_stage_request(token: &str, zip: &[u8]) -> Request<Body> {
    let mut request = multipart_request(
        "POST",
        SETUP_RESTORE_STAGE_URI,
        "",
        &[],
        Some(("file", "backup.zip", zip)),
    );
    request.headers_mut().insert(
        "x-setup-token",
        token.parse().expect("ヘッダを組み立てられなかった"),
    );
    from_loopback(request)
}

/// 入れ直した直後 (admin が0人) に、セットアップの画面からバックアップで戻せる。
#[sqlx::test]
async fn setup_restores_a_backup_instead_of_creating_an_admin(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    let zip = download_backup(app.clone(), &cookie).await;
    sqlx::query("DELETE FROM users")
        .execute(&pool)
        .await
        .expect("ユーザーを消せなかった");
    let token = issue_setup_token(app.clone()).await;

    // トークンが無ければ受け取らない。
    let (status, _) = send(app.clone(), setup_stage_request("", &zip)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = send(app.clone(), setup_stage_request(&token, &zip)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send(
        app.clone(),
        from_loopback(build_request(
            "POST",
            SETUP_RESTORE_URI,
            None,
            Some(&format!(
                r#"{{"token":"{token}","id":"{}"}}"#,
                staged_id(&body)
            )),
        )),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    // 戻したデータの管理者でログインでき、セットアップの口は閉じる。
    login(app.clone(), "admin-alice", "correct-password").await;
    let (status, _) = send(
        app,
        from_loopback(build_request("GET", SETUP_STATUS_URI, None, None)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
