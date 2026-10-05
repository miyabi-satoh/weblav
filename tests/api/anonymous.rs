//! 匿名閲覧 (docs/access.md「公開範囲と匿名閲覧」)

use super::*;

/// 匿名が `public` のグループを辿れることと、子一覧も閲覧レベルで絞られることを確認する。
#[sqlx::test]
async fn browse_group_anonymous_sees_public_group_and_public_children(pool: SqlitePool) {
    let group_id = insert_group_under(&pool, "公開グループ", "public", None).await;
    insert_link_by(&pool, "公開の子", "public", None, Some(group_id)).await;
    // 匿名に見えるグループの中でも、要ログインの子は匿名には見えてはならない。
    insert_link_by(
        &pool,
        "要ログインの子",
        "authenticated",
        None,
        Some(group_id),
    )
    .await;

    let app = test_app(pool).await;
    let (status, body) = send_anon(app, "GET", &format!("/api/v1/contents/{group_id}/group")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("公開の子"), "{body}");
    assert!(!body.contains("要ログインの子"), "{body}");
}

/// 匿名が `authenticated` のグループを要求すると401(ログインすれば見られる)。
#[sqlx::test]
async fn browse_group_anonymous_on_authenticated_group_is_unauthorized(pool: SqlitePool) {
    let group_id = insert_group_under(&pool, "社内グループ", "authenticated", None).await;

    let app = test_app(pool).await;
    let (status, _) = send_anon(app, "GET", &format!("/api/v1/contents/{group_id}/group")).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// 存在しないidは、匿名であっても401ではなく404(→ docs/access.md「匿名閲覧の受け口」の表)。
#[sqlx::test]
async fn browse_group_anonymous_on_missing_id_is_not_found(pool: SqlitePool) {
    let app = test_app(pool).await;
    let (status, _) = send_anon(app, "GET", "/api/v1/contents/999999/group").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// `authenticated` のフォルダに対する匿名のbrowseは401。実体のディレクトリを用意した上で
/// 確認する: レベル判定をファイルシステムアクセスより後に置くと、401と404の差から
/// パスの存在有無を探れてしまうため。
#[sqlx::test]
async fn folder_browse_anonymous_on_authenticated_folder_is_unauthorized(pool: SqlitePool) {
    let dir = temp_test_dir("browse-anon-authenticated");
    std::fs::write(dir.join("ok.txt"), b"hello").expect("failed to write test file");

    let content_id = insert_folder_by(
        &pool,
        "教材",
        dir.display().to_string(),
        "authenticated",
        None,
        None,
    )
    .await;

    let app = test_app(pool).await;
    let (status, _) = send_anon(app, "GET", &format!("/api/v1/contents/{content_id}/browse")).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// 存在しないファイルを指したbrowseでも、レベル不足なら先に401になる(存在の有無を
/// 401/404の差で漏らさない)。
#[sqlx::test]
async fn folder_browse_anonymous_on_authenticated_folder_hides_missing_path(pool: SqlitePool) {
    let dir = temp_test_dir("browse-anon-missing-path");

    let content_id = insert_folder_by(
        &pool,
        "教材",
        dir.display().to_string(),
        "authenticated",
        None,
        None,
    )
    .await;

    let app = test_app(pool).await;
    let (status, _) = send_anon(
        app,
        "GET",
        &format!("/api/v1/contents/{content_id}/browse?path=no-such-dir"),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// `public` のフォルダは匿名でも一覧・ダウンロードできる。
#[sqlx::test]
async fn folder_browse_and_download_anonymous_on_public_folder_succeeds(pool: SqlitePool) {
    let dir = temp_test_dir("browse-anon-public");
    std::fs::write(dir.join("note.txt"), b"hello").expect("failed to write test file");

    let content_id = insert_folder_by(
        &pool,
        "教材",
        dir.display().to_string(),
        "public",
        None,
        None,
    )
    .await;

    let app = test_app(pool).await;

    let (browse_status, body) = send_anon(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{content_id}/browse"),
    )
    .await;
    assert_eq!(browse_status, StatusCode::OK);
    assert!(body.contains("note.txt"), "{body}");

    let (download_status, _) = send_anon(
        app,
        "GET",
        &format!("/api/v1/contents/{content_id}/download?path=note.txt"),
    )
    .await;
    assert_eq!(download_status, StatusCode::OK);
}

/// `authenticated` のフォルダ配下のファイルは、匿名からのダウンロードで401になる。
#[sqlx::test]
async fn folder_download_anonymous_on_authenticated_folder_is_unauthorized(pool: SqlitePool) {
    let dir = temp_test_dir("download-anon-authenticated");
    std::fs::write(dir.join("note.txt"), b"hello").expect("failed to write test file");

    let content_id = insert_folder_by(
        &pool,
        "教材",
        dir.display().to_string(),
        "authenticated",
        None,
        None,
    )
    .await;

    let app = test_app(pool).await;
    let (status, _) = send_anon(
        app,
        "GET",
        &format!("/api/v1/contents/{content_id}/download?path=note.txt"),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// ブラウザが直接開いたとき (`Accept: text/html`) は、JSON ではなくログイン画面へ送る。
/// 生の envelope をタブに出すと、そこから戻る手がかりが無くなるため (→ `api::browser`)。
#[sqlx::test]
async fn folder_download_from_a_browser_redirects_to_login(pool: SqlitePool) {
    let dir = temp_test_dir("download-browser-login");
    std::fs::write(dir.join("note.txt"), b"hello").expect("テスト用ファイルを作れなかった");

    let content_id = insert_folder_by(
        &pool,
        "教材",
        dir.display().to_string(),
        "authenticated",
        None,
        None,
    )
    .await;

    let app = test_app(pool).await;
    let response = send_raw(
        app,
        Request::get(format!(
            "/api/v1/contents/{content_id}/download?path=note.txt"
        ))
        .header(header::ACCEPT, "text/html,application/xhtml+xml,*/*;q=0.8")
        .body(Body::empty())
        .expect("リクエストを組み立てられなかった"),
    )
    .await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response
        .headers()
        .get(header::LOCATION)
        .expect("Locationが無い")
        .to_str()
        .expect("Locationを文字列にできなかった");
    // ログインしたら開こうとしていたファイルへ戻せること。
    // **`/api/v1` を含む完全なパスであること**を見る。`Router::nest` はネストした
    // ルーターに渡す URI からプレフィックスを落とすため、`OriginalUri` を取り損ねると
    // ここが `/contents/...` になり、ログイン後に開けないパスを渡してしまう。
    assert_eq!(
        location,
        format!(
            "/login?redirect=%2Fapi%2Fv1%2Fcontents%2F{content_id}%2Fdownload%3Fpath%3Dnote%2Etxt"
        ),
        "戻り先が完全なパスになっていない"
    );
}

/// 見つからないものをブラウザが開いたときは、理由を載せてトップへ送る。
#[sqlx::test]
async fn download_of_a_missing_content_from_a_browser_redirects_to_top(pool: SqlitePool) {
    let app = test_app(pool).await;
    let response = send_raw(
        app,
        Request::get("/api/v1/contents/999/download")
            .header(header::ACCEPT, "text/html")
            .body(Body::empty())
            .expect("リクエストを組み立てられなかった"),
    )
    .await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response
            .headers()
            .get(header::LOCATION)
            .expect("Locationが無い")
            .to_str()
            .expect("Locationを文字列にできなかった"),
        "/?error=not_found"
    );
}

/// `fetch` から呼ぶ API クライアントには、今までどおり JSON の envelope を返す。
#[sqlx::test]
async fn download_errors_stay_json_for_api_clients(pool: SqlitePool) {
    let app = test_app(pool).await;
    let response = send_raw(
        app,
        Request::get("/api/v1/contents/999/download")
            .header(header::ACCEPT, "*/*")
            .body(Body::empty())
            .expect("リクエストを組み立てられなかった"),
    )
    .await;

    let status = response.status();
    let body = body_string(response).await;
    assert_error(status, &body, StatusCode::NOT_FOUND, "not_found");
}

/// ダウンロード対象が存在しないidなら、匿名でも404。
#[sqlx::test]
async fn download_anonymous_on_missing_id_is_not_found(pool: SqlitePool) {
    let app = test_app(pool).await;
    let (status, _) = send_anon(app, "GET", "/api/v1/contents/999999/download").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// 配下のサブディレクトリ・ファイル一覧を取得できる。ドットファイルは除外される。
#[sqlx::test]
async fn folder_browse_lists_entries_and_excludes_dotfiles(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let dir = temp_test_dir("browse-list");
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub").join("file.txt"), b"hello").unwrap();
    std::fs::write(dir.join(".DS_Store"), b"junk").unwrap();

    let content_id = insert_folder_by(
        &pool,
        "教材",
        dir.display().to_string(),
        "authenticated",
        None,
        None,
    )
    .await;

    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let (status, body) = send_empty(
        app,
        "GET",
        &format!("/api/v1/contents/{content_id}/browse"),
        &cookie,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"sub\""), "{body}");
    assert!(body.contains(r#""isDir":true"#), "{body}");
    assert!(!body.contains("DS_Store"), "{body}");
}

/// `sort=new` はファイルの更新日時の降順になり、ディレクトリ優先の並びは変わらない
/// (→ docs/ui.md「ホーム・グループ・フォルダ・アーカイブの並び順」)。
#[sqlx::test]
async fn folder_browse_accepts_sort_query(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let dir = temp_test_dir("browse-sort");
    std::fs::create_dir_all(dir.join("sub")).expect("サブディレクトリを作れなかった");
    std::fs::write(dir.join("b.txt"), b"newer").expect("b.txt を書けなかった");
    std::fs::write(dir.join("a.txt"), b"older").expect("a.txt を書けなかった");

    let older = std::time::SystemTime::now() - std::time::Duration::from_secs(120);
    let newer = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
    // Windows では読み取り専用で開いたハンドルに set_modified すると
    // PermissionDenied になるため、書き込み権限つきで開く。
    std::fs::OpenOptions::new()
        .write(true)
        .open(dir.join("a.txt"))
        .expect("a.txt を開けなかった")
        .set_modified(older)
        .expect("a.txt の更新日時を変えられなかった");
    std::fs::OpenOptions::new()
        .write(true)
        .open(dir.join("b.txt"))
        .expect("b.txt を開けなかった")
        .set_modified(newer)
        .expect("b.txt の更新日時を変えられなかった");

    let content_id = insert_folder_by(
        &pool,
        "教材",
        dir.display().to_string(),
        "authenticated",
        None,
        None,
    )
    .await;

    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    async fn names(app: Router, uri: &str, cookie: &str) -> Vec<String> {
        let (status, body) = send_empty(app, "GET", uri, cookie).await;
        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body).expect("JSONとして読めなかった");
        json["entries"]
            .as_array()
            .expect("entriesが配列でなかった")
            .iter()
            .map(|entry| {
                entry["name"]
                    .as_str()
                    .expect("nameが文字列でなかった")
                    .to_string()
            })
            .collect()
    }

    let uri = format!("/api/v1/contents/{content_id}/browse");
    // 既定 (タイトル順)。ディレクトリが先頭、その後は数字を含まない名前順。
    assert_eq!(
        names(app.clone(), &uri, &cookie).await,
        vec!["sub", "a.txt", "b.txt"]
    );
    // `sort=new`。ディレクトリは変わらず先頭、ファイルは更新日時の降順。
    assert_eq!(
        names(app, &format!("{uri}?sort=new"), &cookie).await,
        vec!["sub", "b.txt", "a.txt"]
    );
}

/// 壊れたシンボリックリンク(リンク先が存在しない)が1本混ざっていても、一覧取得自体は
/// 失敗せず(500にならず)、他の正当なエントリは列挙されることを確認する
/// (`entry.metadata()` はリンク先を辿るため、対策していないとここで失敗する)。
#[cfg(unix)]
#[sqlx::test]
async fn folder_browse_tolerates_dangling_symlink(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let dir = temp_test_dir("browse-dangling-symlink");
    std::fs::write(dir.join("ok.txt"), b"hello").unwrap();
    std::os::unix::fs::symlink("/nonexistent-target-for-test", dir.join("dangling")).unwrap();

    let content_id = insert_folder_by(
        &pool,
        "教材",
        dir.display().to_string(),
        "authenticated",
        None,
        None,
    )
    .await;

    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let (status, body) = send_empty(
        app,
        "GET",
        &format!("/api/v1/contents/{content_id}/browse"),
        &cookie,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"ok.txt\""), "{body}");
    assert!(!body.contains("dangling"), "{body}");
}

/// `..` を含む相対パス・絶対パス指定によるパストラバーサルは404になる
/// (403にしない=パスの存在有無を教えない)。
#[sqlx::test]
async fn folder_browse_rejects_path_traversal(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let dir = temp_test_dir("browse-traversal");

    let content_id = insert_folder_by(
        &pool,
        "教材",
        dir.display().to_string(),
        "authenticated",
        None,
        None,
    )
    .await;

    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    for path in ["../", "..", "a/../../b", "/etc/passwd"] {
        let (status, _) = send_empty(
            app.clone(),
            "GET",
            &format!("/api/v1/contents/{content_id}/browse?path={path}"),
            &cookie,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "path={path}");
    }
}

/// シンボリックリンクでroot外を指すエントリへのアクセスも404になる
/// (`..`によるトラバーサルだけでなくシンボリックリンクによる脱出も防げることの確認)。
/// 加えて、一覧(browseのルート)にはそのシンボリックリンク自体が現れないことも確認する
/// (一覧に出すとroot外ファイルの名前・サイズ・更新日時が漏れてしまうため、
/// `read_entries`はシンボリックリンクを列挙しない方針)。
#[cfg(unix)]
#[sqlx::test]
async fn folder_browse_rejects_symlink_escape(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let root = temp_test_dir("symlink-root");
    let outside = temp_test_dir("symlink-outside");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), b"secret").unwrap();
    std::fs::write(root.join("ok.txt"), b"hello").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();

    let content_id = insert_folder_by(
        &pool,
        "教材",
        root.display().to_string(),
        "authenticated",
        None,
        None,
    )
    .await;

    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let (list_status, list_body) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{content_id}/browse"),
        &cookie,
    )
    .await;
    assert_eq!(list_status, StatusCode::OK);
    assert!(list_body.contains("\"ok.txt\""), "{list_body}");
    assert!(!list_body.contains("escape"), "{list_body}");

    let (status, _) = send_empty(
        app,
        "GET",
        &format!("/api/v1/contents/{content_id}/browse?path=escape"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// `.html` ファイルのダウンロードは `Content-Disposition: attachment` になる
/// (同一オリジンでのインライン実行によるXSS対策)。
#[sqlx::test]
async fn folder_download_forces_attachment_for_html(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let dir = temp_test_dir("download-html");
    std::fs::write(dir.join("page.html"), b"<script>alert(1)</script>").unwrap();

    let content_id = insert_folder_by(
        &pool,
        "教材",
        dir.display().to_string(),
        "authenticated",
        None,
        None,
    )
    .await;

    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let response = send_raw(
        app,
        build_request(
            "GET",
            &format!("/api/v1/contents/{content_id}/download?path=page.html"),
            Some(&cookie),
            None,
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let disposition = response
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .to_string();
    assert!(disposition.starts_with("attachment"), "{disposition}");
    assert_eq!(
        response
            .headers()
            .get(header::X_CONTENT_TYPE_OPTIONS)
            .and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );
}

/// PDFファイルのダウンロードは `Content-Disposition: inline`(ホワイトリスト対象)。
#[sqlx::test]
async fn folder_download_allows_inline_for_pdf(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let dir = temp_test_dir("download-pdf");
    std::fs::write(dir.join("doc.pdf"), b"%PDF-1.4 fake pdf content").unwrap();

    let content_id = insert_folder_by(
        &pool,
        "教材",
        dir.display().to_string(),
        "authenticated",
        None,
        None,
    )
    .await;

    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let response = send_raw(
        app,
        build_request(
            "GET",
            &format!("/api/v1/contents/{content_id}/download?path=doc.pdf"),
            Some(&cookie),
            None,
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let disposition = response
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .to_string();
    assert!(disposition.starts_with("inline"), "{disposition}");
}

/// 音声ファイルはgzip圧縮の対象外(`CompressionLayer::compress_when`)であるべき。
/// gzipにするとRange/Content-Lengthが失われ、既に圧縮済みの音声を再圧縮するだけ無駄になる。
/// `SizeAbove`の既定閾値(32バイト)に埋もれないよう、それを超えるサイズのファイルで検証する。
#[sqlx::test]
async fn folder_download_of_audio_is_not_gzip_compressed(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let dir = temp_test_dir("download-audio");
    std::fs::write(dir.join("track.mp3"), vec![0u8; 64]).unwrap();

    let content_id = insert_folder_by(
        &pool,
        "教材",
        dir.display().to_string(),
        "authenticated",
        None,
        None,
    )
    .await;

    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let response = send_raw(
        app,
        Request::get(format!(
            "/api/v1/contents/{content_id}/download?path=track.mp3"
        ))
        .header(header::COOKIE, &cookie)
        .header(header::ACCEPT_ENCODING, "gzip")
        .body(Body::empty())
        .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers().get(header::CONTENT_ENCODING).is_none(),
        "音声ファイルがgzip圧縮され、Range/Content-Lengthが失われている"
    );
}

/// 画像はtower-httpの既定predicate(`DefaultPredicate`)でもgzip対象外。
/// `compress_when`は既定predicateを丸ごと差し替えるAPIのため、音声等を除外に
/// 追加する際に既定の除外(gRPC/image/SSE)まで消してしまう回帰が起きやすい。
#[sqlx::test]
async fn folder_download_of_image_is_not_gzip_compressed(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let dir = temp_test_dir("download-image");
    std::fs::write(dir.join("photo.png"), vec![0u8; 64]).unwrap();

    let content_id = insert_folder_by(
        &pool,
        "教材",
        dir.display().to_string(),
        "authenticated",
        None,
        None,
    )
    .await;

    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let response = send_raw(
        app,
        Request::get(format!(
            "/api/v1/contents/{content_id}/download?path=photo.png"
        ))
        .header(header::COOKIE, &cookie)
        .header(header::ACCEPT_ENCODING, "gzip")
        .body(Body::empty())
        .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers().get(header::CONTENT_ENCODING).is_none(),
        "画像がgzip圧縮されている(既定predicateの除外が壊れている)"
    );
}
