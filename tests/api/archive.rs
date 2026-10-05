//! アーカイブ型コンテンツの作成と再スキャン。

use super::*;

#[sqlx::test]
async fn archive_content_create_by_admin_normalizes_extensions(pool: SqlitePool) {
    let dir = temp_test_dir("archive-create");
    let canonical_dir = dir
        .canonicalize()
        .expect("登録先をcanonicalizeできなかった");

    let (app, cookie) = admin_app(&pool).await;

    let path_json = json_string(&dir.display().to_string());
    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/contents",
        &cookie,
        &format!(
            r#"{{"type":"archive","title":"英検 過去問","path":{path_json},"extensions":".MP3, pdf ,,"}}"#
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    let created: serde_json::Value =
        serde_json::from_str(&body).expect("作成レスポンスをJSONとして読めなかった");
    assert_eq!(created["type"], "archive", "{body}");
    assert_eq!(
        created["path"].as_str().expect("pathが返らなかった"),
        canonical_path_as_api_returns_it(&canonical_dir),
        "{body}"
    );
    assert_eq!(created["extensions"], "mp3,pdf", "{body}");
}

/// **roots 封じ込めが唯一の認可障壁**になったので、編集者でも外は 422 になることを確かめる。
/// 作成・更新の両方を見る (種別による admin 判定が無くなり、更新経路の検査が薄くなったため)。
#[sqlx::test]
async fn directory_contents_by_regular_user_are_confined_to_the_shared_folders(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    // `temp_test_dir` の外。土台として登録してあるのはその親だけ (→ `register_content_root`)。
    let outside = temp_root_dir("confined-outside");
    let inside = temp_test_dir("confined-inside");

    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;
    let json = |dir: &std::path::Path| json_string(&dir.display().to_string());

    // 作成: 外は 422。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/contents",
        &cookie,
        &format!(
            r#"{{"type":"folder","title":"教材","path":{}}}"#,
            json(&outside)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // 作成: 中は通る。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/contents",
        &cookie,
        &format!(
            r#"{{"type":"folder","title":"教材","path":{}}}"#,
            json(&inside)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = extract_id(&body);

    // 更新: 外へ付け替えられない。
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}"),
        &cookie,
        &format!(
            r#"{{"type":"folder","title":"教材","path":{}}}"#,
            json(&outside)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // ディレクトリ選択APIも、編集者から見て外は同じ 422 になる。
    for uri in [
        format!("/api/v1/admin/fs/dirs?path={}", outside.display()),
        format!("/api/v1/admin/fs/count?path={}", outside.display()),
    ] {
        let (status, _) = send_empty(app.clone(), "GET", &uri, &cookie).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{uri}");
    }
}

/// 存在しないパスとルートの外を、編集者に区別させない (→ docs/folders.md「公開できるフォルダ」)。
/// 区別が残ると、任意の絶対パスの存在を確かめる道具になる。
#[sqlx::test]
async fn fs_dirs_does_not_tell_a_regular_user_whether_a_path_outside_exists(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let existing = temp_root_dir("oracle-existing");
    let missing = temp_root_path("oracle-missing");

    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let mut bodies = Vec::new();
    for dir in [existing.path(), missing.as_path()] {
        let response = send_raw(
            app.clone(),
            build_request(
                "GET",
                &format!("/api/v1/admin/fs/dirs?path={}", dir.display()),
                Some(&cookie),
                None,
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        bodies.push(body_string(response).await);
    }
    assert_eq!(bodies[0], bodies[1], "{bodies:?}");
}

/// 表示タイトルのテンプレートは、アーカイブの作成者と `admin` が変えられる
/// (→ docs/access.md「ロールと操作」)。ほかの `user` は、同じ値を送り返す更新も 403。
#[sqlx::test]
async fn title_template_can_be_changed_by_the_creator_and_an_admin(pool: SqlitePool) {
    insert_user(&pool, "bob", "correct-password").await;
    insert_user(&pool, "carol", "correct-password").await;
    let dir = temp_test_dir("title-template-guard");

    let app = test_app(pool.clone()).await;
    let admin_cookie = admin_cookie(app.clone(), &pool).await;
    let bob_cookie = login(app.clone(), "bob", "correct-password").await;
    let id = create_archive(app.clone(), &bob_cookie, &dir, None).await;
    let path_json = json_string(&dir.display().to_string());
    let put = |cookie: String, template: &str| {
        let body = format!(
            r#"{{"type":"archive","title":"英検 過去問","path":{path_json},"titleTemplate":{}}}"#,
            serde_json::to_string(template).expect("テンプレートをJSON文字列にできなかった")
        );
        let app = app.clone();
        async move {
            send_json(
                app,
                "PUT",
                &format!("/api/v1/contents/{id}"),
                &cookie,
                &body,
            )
            .await
        }
    };

    let (status, body) = put(bob_cookie, "リスニング").await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = put(admin_cookie, "リーディング").await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let carol_cookie = login(app.clone(), "carol", "correct-password").await;
    let (status, body) = put(carol_cookie, "リーディング").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

/// 編集者もアーカイブを作れる (→ docs/access.md「ロールと操作」)。
#[sqlx::test]
async fn archive_content_create_by_regular_user_succeeds(pool: SqlitePool) {
    insert_user(&pool, "bob", "correct-password").await;
    let dir = temp_test_dir("archive-by-user");

    let app = test_app(pool).await;
    let cookie = login(app.clone(), "bob", "correct-password").await;

    let path_json = json_string(&dir.display().to_string());
    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/contents",
        &cookie,
        &format!(r#"{{"type":"archive","title":"英検 過去問","path":{path_json}}}"#),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// 走査は再帰的にファイルを拾い、ドット始まりを除き、拡張子で絞り込む。
/// 新しく見つかったアイテムは非公開から始まる (→ docs/archive.md「アイテムの公開」)。
#[sqlx::test]
async fn archive_rescan_indexes_files_recursively_as_unpublished(pool: SqlitePool) {
    insert_admin(&pool, "admin-alice", "correct-password").await;
    let dir = temp_test_dir("archive-rescan");
    let sub = dir.join("2024").join("第1回");
    std::fs::create_dir_all(&sub).expect("サブディレクトリを作れなかった");
    std::fs::write(sub.join("listening.mp3"), b"").expect("ファイルを作れなかった");
    std::fs::write(sub.join("script.pdf"), b"").expect("ファイルを作れなかった");
    std::fs::write(dir.join(".DS_Store"), b"").expect("ファイルを作れなかった");
    std::fs::write(dir.join("readme.txt"), b"").expect("ファイルを作れなかった");

    let app = test_app(pool.clone()).await;
    let cookie = login(app.clone(), "admin-alice", "correct-password").await;
    let id = create_archive(app.clone(), &cookie, &dir, Some("mp3,pdf")).await;

    let (status, body) = rescan(app, &cookie, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let result: serde_json::Value =
        serde_json::from_str(&body).expect("結果をJSONとして読めなかった");
    assert_eq!(result["added"], 2, "{body}");
    assert_eq!(result["removed"], 0, "{body}");
    assert_eq!(result["total"], 2, "{body}");

    let rows = sqlx::query!(
        "SELECT rel_path, published FROM archive_items WHERE archive_id = ? ORDER BY rel_path",
        id
    )
    .fetch_all(&pool)
    .await
    .expect("索引を読めなかった");
    let rel_paths: Vec<&str> = rows.iter().map(|row| row.rel_path.as_str()).collect();
    assert_eq!(
        rel_paths,
        vec!["2024/第1回/listening.mp3", "2024/第1回/script.pdf"]
    );
    assert!(rows.iter().all(|row| row.published == 0));
}

/// 別々のアーカイブの再スキャンが同時に重なっても、データベースのロックで失敗しない。
/// 同期は読んでから書くので、書き込みのロックを後から取りに行くと
/// `database is locked` (503) になる (→ `db::begin_write`)。
/// 本番と同じ `db::connect` (WAL のファイルDB) で確かめる。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_rescans_of_different_archives_do_not_fail_with_database_locked() {
    // 重なる時間を長くするため、1回の同期で書く行を多めにする。
    const ARCHIVES: usize = 6;
    const FILES_PER_ARCHIVE: usize = 200;

    // 途中で assert が落ちても、Drop で DB と登録先を片付ける。
    let temp = test_support::project_temp_dir("api-test", "concurrent-rescan");
    let base = temp.path();
    let pool = weblav::db::connect(&base.join("db").join("weblav.db"))
        .await
        .expect("DBに接続できなかった");
    weblav::db::migrate(&pool)
        .await
        .expect("マイグレーションを適用できなかった");
    insert_admin(&pool, "admin-alice", "correct-password").await;
    // 登録先をこの実行専用の場所に作るので、そこも「公開できるフォルダ」に入れる。
    register_root(&pool, base).await;

    let app = test_app_pro(pool.clone()).await;
    let cookie = login(app.clone(), "admin-alice", "correct-password").await;

    let mut ids = Vec::with_capacity(ARCHIVES);
    for n in 0..ARCHIVES {
        let dir = base.join(format!("archive-{n}"));
        std::fs::create_dir_all(&dir).expect("登録先を作れなかった");
        for file in 0..FILES_PER_ARCHIVE {
            std::fs::write(dir.join(format!("{file:03}.mp3")), b"")
                .expect("ファイルを作れなかった");
        }
        ids.push(create_archive(app.clone(), &cookie, &dir, Some("mp3")).await);
    }

    let mut rescans = tokio::task::JoinSet::new();
    for id in ids {
        let app = app.clone();
        let cookie = cookie.clone();
        rescans.spawn(async move { rescan(app, &cookie, id).await });
    }
    while let Some(joined) = rescans.join_next().await {
        let (status, body) = joined.expect("再スキャンのタスクが途中で落ちた");
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    // Windows は開いているファイルを消せないので、片付けの前に接続を閉じる。
    pool.close().await;
}

/// 再スキャンは既存アイテムの公開フラグを保ち、消えたアイテムの行を削除する。
/// 行を消すことが「一度対象から外れたら公開フラグを復元しない」の実装
/// (→ docs/archive.md「スキャン」)。
#[sqlx::test]
async fn archive_rescan_keeps_published_flags_and_removes_vanished_items(pool: SqlitePool) {
    let dir = temp_test_dir("archive-resync");
    std::fs::write(dir.join("keep.mp3"), b"").expect("ファイルを作れなかった");
    std::fs::write(dir.join("gone.mp3"), b"").expect("ファイルを作れなかった");

    let (app, cookie, id) = setup_archive(&pool, &dir, None).await;

    sqlx::query!(
        "UPDATE archive_items SET published = 1 WHERE archive_id = ? AND rel_path = 'keep.mp3'",
        id
    )
    .execute(&pool)
    .await
    .expect("公開フラグを立てられなかった");

    std::fs::remove_file(dir.join("gone.mp3")).expect("ファイルを消せなかった");
    std::fs::write(dir.join("new.mp3"), b"").expect("ファイルを作れなかった");

    let (status, body) = rescan(app, &cookie, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let result: serde_json::Value =
        serde_json::from_str(&body).expect("結果をJSONとして読めなかった");
    assert_eq!(result["added"], 1, "{body}");
    assert_eq!(result["removed"], 1, "{body}");
    assert_eq!(result["total"], 2, "{body}");

    let rows = sqlx::query!(
        "SELECT rel_path, published FROM archive_items WHERE archive_id = ? ORDER BY rel_path",
        id
    )
    .fetch_all(&pool)
    .await
    .expect("索引を読めなかった");
    let published: Vec<(&str, i64)> = rows
        .iter()
        .map(|row| (row.rel_path.as_str(), row.published))
        .collect();
    assert_eq!(published, vec![("keep.mp3", 1), ("new.mp3", 0)]);
}

/// 拡張子の変更では索引を消さない。対象であり続けたアイテムは公開フラグを保ち、
/// 対象から外れた行だけが次の走査で消える (→ docs/archive.md「スキャン」)。
#[sqlx::test]
async fn archive_extensions_change_keeps_flags_of_still_matching_items(pool: SqlitePool) {
    let dir = temp_test_dir("archive-ext-change");
    std::fs::write(dir.join("a.mp3"), b"").expect("ファイルを作れなかった");
    std::fs::write(dir.join("b.pdf"), b"").expect("ファイルを作れなかった");

    let (app, cookie, id) = setup_archive(&pool, &dir, None).await;
    sqlx::query!(
        "UPDATE archive_items SET published = 1 WHERE archive_id = ?",
        id
    )
    .execute(&pool)
    .await
    .expect("公開フラグを立てられなかった");

    let path_json = json_string(&dir.display().to_string());
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}"),
        &cookie,
        &format!(r#"{{"title":"英検 過去問","path":{path_json},"extensions":"mp3"}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = rescan(app, &cookie, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let result: serde_json::Value =
        serde_json::from_str(&body).expect("結果をJSONとして読めなかった");
    assert_eq!(result["added"], 0, "{body}");
    assert_eq!(result["removed"], 1, "{body}");

    let rows = sqlx::query!(
        "SELECT rel_path, published FROM archive_items WHERE archive_id = ?",
        id
    )
    .fetch_all(&pool)
    .await
    .expect("索引を読めなかった");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].rel_path, "a.mp3");
    assert_eq!(
        rows[0].published, 1,
        "対象であり続けたアイテムの公開フラグは保つ"
    );
}

/// 登録先を変えたら索引を全削除する。`rel_path` の起点が変わるため、行を残すと
/// 変更前の公開フラグが無関係なファイルへ引き継がれる (→ docs/archive.md「スキャン」)。
#[sqlx::test]
async fn archive_path_change_clears_indexed_items(pool: SqlitePool) {
    let old_dir = temp_test_dir("archive-path-old");
    let new_dir = temp_test_dir("archive-path-new");
    std::fs::create_dir_all(&old_dir).expect("登録先を作れなかった");
    std::fs::create_dir_all(&new_dir).expect("登録先を作れなかった");
    std::fs::write(old_dir.join("a.mp3"), b"").expect("ファイルを作れなかった");

    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &old_dir, None).await;
    let (status, body) = rescan(app.clone(), &cookie, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    sqlx::query!(
        "UPDATE archive_items SET published = 1 WHERE archive_id = ?",
        id
    )
    .execute(&pool)
    .await
    .expect("公開フラグを立てられなかった");

    let path_json = json_string(&new_dir.display().to_string());
    let (status, body) = send_json(
        app,
        "PUT",
        &format!("/api/v1/contents/{id}"),
        &cookie,
        &format!(r#"{{"title":"英検 過去問","path":{path_json}}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let remaining = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM archive_items WHERE archive_id = ?",
        id
    )
    .fetch_one(&pool)
    .await
    .expect("索引を数えられなかった");
    assert_eq!(remaining, 0);
}

/// 走査はシンボリックリンクを索引しない。`follow_links(false)` はリンク先へ再帰
/// しないだけで、リンク自体をエントリとして拾わない意味ではない (→ docs/archive.md「スキャン」)。
#[cfg(unix)]
#[sqlx::test]
async fn archive_rescan_excludes_symlinks(pool: SqlitePool) {
    let dir = temp_test_dir("archive-symlink");
    let outside = temp_test_dir("archive-symlink-outside");
    std::fs::create_dir_all(&dir).expect("登録先を作れなかった");
    std::fs::create_dir_all(&outside).expect("外部ディレクトリを作れなかった");
    std::fs::write(dir.join("real.mp3"), b"").expect("ファイルを作れなかった");
    std::fs::write(outside.join("secret.mp3"), b"").expect("ファイルを作れなかった");
    std::os::unix::fs::symlink(outside.join("secret.mp3"), dir.join("link.mp3"))
        .expect("シンボリックリンクを作れなかった");

    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;

    let (status, body) = rescan(app, &cookie, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let rows = sqlx::query_scalar!(
        "SELECT rel_path FROM archive_items WHERE archive_id = ?",
        id
    )
    .fetch_all(&pool)
    .await
    .expect("索引を読めなかった");
    assert_eq!(rows, vec!["real.mp3".to_string()]);
}

/// アーカイブ以外のidを指した再スキャンは404。存在しないidと区別しない。
#[sqlx::test]
async fn archive_rescan_on_non_archive_content_is_not_found(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/contents",
        &cookie,
        r#"{"type":"link","title":"外部","url":"https://example.com"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = extract_id(&body);

    let (status, body) = rescan(app, &cookie, id).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}
