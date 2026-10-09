//! 公開範囲の継承 (docs/access.md「祖先のグループを辿る」・「親が外れるときの公開範囲」)

use super::*;

/// 保存されている公開範囲を読む。
async fn stored_visibility(pool: &SqlitePool, id: i64) -> String {
    sqlx::query_scalar("SELECT visibility FROM contents WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("公開範囲を読めなかった")
}

/// 親グループより緩い公開範囲でも保存できる。グループの公開範囲も、子によらず変えられる。
#[sqlx::test]
async fn content_looser_than_its_group_can_be_saved(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let alice_id = db_user_id(&pool, "alice").await;
    let group_id = insert_group_under(&pool, "準備中", "hidden", None).await;
    sqlx::query("UPDATE contents SET created_by = ? WHERE id = ?")
        .bind(alice_id)
        .bind(group_id)
        .execute(&pool)
        .await
        .expect("failed to set the creator");
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    // 公開範囲を省略すると既定の public になる。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/contents",
        &cookie,
        &format!(
            r#"{{"type":"link","title":"リンク","url":"https://example.com","parentId":{group_id}}}"#
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(body.contains(r#""visibility":"public""#), "{body}");

    for visibility in ["authenticated", "public", "hidden"] {
        let (status, body) = send_json(
            app.clone(),
            "PUT",
            &format!("/api/v1/contents/{group_id}"),
            &cookie,
            &format!(r#"{{"title":"準備中","visibility":"{visibility}"}}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{visibility}: {body}");
    }
}

/// multipart の作成も、親グループより緩い公開範囲で保存できる。
#[sqlx::test]
async fn upload_content_looser_than_its_group_can_be_saved(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let group_id = insert_group_under(&pool, "職員用", "authenticated", None).await;
    let (app, _blobs_dir) = test_app_with_blobs(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let response = app
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "資料"), ("parentId", &group_id.to_string())],
            Some(("file", "doc.txt", b"hello")),
        ))
        .await
        .expect("request should not fail");

    assert_eq!(response.status(), StatusCode::CREATED);
    let body = body_string(response).await;
    assert!(body.contains(r#""visibility":"public""#), "{body}");
}

/// 祖先のグループを開けなければ、中のグループも開けない。親だけでなく、その上の祖先も見る。
#[sqlx::test]
async fn group_inside_restricted_ancestor_is_unauthorized_anonymously(pool: SqlitePool) {
    insert_user(&pool, "bob", "correct-password").await;
    let staff = insert_group_under(&pool, "職員用", "authenticated", None).await;
    let inner = insert_group_under(&pool, "資料", "public", Some(staff)).await;
    let nested = insert_group_under(&pool, "今年度", "public", Some(inner)).await;
    let app = test_app(pool).await;

    for id in [inner, nested] {
        let uri = format!("/api/v1/contents/{id}/group");
        let (status, body) = send_anon(app.clone(), "GET", &uri).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{id}: {body}");
    }

    let cookie = login(app.clone(), "bob", "correct-password").await;
    for id in [inner, nested] {
        let uri = format!("/api/v1/contents/{id}/group");
        let (status, body) = send_empty(app.clone(), "GET", &uri, &cookie).await;
        assert_eq!(status, StatusCode::OK, "{id}: {body}");
    }
}

/// フォルダーの閲覧・配信も、祖先のグループを開けなければ401になる。
#[sqlx::test]
async fn folder_inside_authenticated_group_is_unauthorized_anonymously(pool: SqlitePool) {
    let dir = temp_test_dir("browse-anon-inherited");
    std::fs::write(dir.join("note.txt"), b"hello").expect("failed to write test file");
    let group_id = insert_group_under(&pool, "職員用", "authenticated", None).await;
    let folder_id = insert_folder_by(
        &pool,
        "教材",
        dir.display().to_string(),
        "public",
        None,
        Some(group_id),
    )
    .await;
    let app = test_app(pool).await;

    for uri in [
        format!("/api/v1/contents/{folder_id}/browse"),
        format!("/api/v1/contents/{folder_id}/download?path=note.txt"),
    ] {
        let (status, body) = send_anon(app.clone(), "GET", &uri).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}: {body}");
    }
}

/// ファイルの配信も、祖先のグループを開けなければ401になる。ログイン済みなら開ける。
#[sqlx::test]
async fn file_inside_authenticated_group_is_unauthorized_anonymously(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let group_id = insert_group_under(&pool, "職員用", "authenticated", None).await;
    let (app, _blobs_dir) = test_app_with_blobs(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let response = app
        .clone()
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "資料"), ("parentId", &group_id.to_string())],
            Some(("file", "doc.txt", b"hello")),
        ))
        .await
        .expect("request should not fail");
    assert_eq!(response.status(), StatusCode::CREATED);
    let uri = format!("/api/v1/contents/{}/download", created_id(response).await);

    let (status, body) = send_anon(app.clone(), "GET", &uri).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, body) = send_empty(app, "GET", &uri, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// アーカイブの閲覧も、祖先のグループを開けなければ401になる。
#[sqlx::test]
async fn archive_inside_authenticated_group_is_unauthorized_anonymously(pool: SqlitePool) {
    let group_id = insert_group_under(&pool, "職員用", "authenticated", None).await;
    let dir = temp_test_dir("archive-view-inherited");
    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;

    let path_json = json_string(&dir.display().to_string());
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}"),
        &cookie,
        &format!(
            r#"{{"title":"英検 過去問","path":{path_json},"parentId":{group_id},"visibility":"public"}}"#
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = view_archive(app, "", id, &[]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
}

/// 祖先が `hidden` のグループの中身は、ログイン済みなら公開範囲によらず並ぶ。
/// ただし他人の `private` は `admin` にしか並ばない (→ docs/access.md「匿名閲覧の受け口」)。
#[sqlx::test]
async fn group_under_hidden_ancestor_lists_contents_except_others_private(pool: SqlitePool) {
    insert_admin(&pool, "admin", "correct-password").await;
    insert_user(&pool, "alice", "correct-password").await;
    insert_user(&pool, "bob", "correct-password").await;
    let alice_id = db_user_id(&pool, "alice").await;
    let drafts = insert_group_under(&pool, "準備中", "hidden", None).await;
    let chapter = insert_group_under(&pool, "第1章", "public", Some(drafts)).await;
    insert_link_by(&pool, "公開の資料", "public", None, Some(chapter)).await;
    insert_link_by(&pool, "非表示の資料", "hidden", None, Some(chapter)).await;
    insert_link_by(
        &pool,
        "アリスのメモ",
        "private",
        Some(alice_id),
        Some(chapter),
    )
    .await;
    let app = test_app(pool).await;
    let uri = format!("/api/v1/contents/{chapter}/group");

    let bob_cookie = login(app.clone(), "bob", "correct-password").await;
    let (status, body) = send_empty(app.clone(), "GET", &uri, &bob_cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("公開の資料"), "{body}");
    assert!(body.contains("非表示の資料"), "{body}");
    assert!(!body.contains("アリスのメモ"), "{body}");

    for username in ["alice", "admin"] {
        let cookie = login(app.clone(), username, "correct-password").await;
        let (status, body) = send_empty(app.clone(), "GET", &uri, &cookie).await;
        assert_eq!(status, StatusCode::OK, "{username}: {body}");
        assert!(body.contains("アリスのメモ"), "{username}: {body}");
    }
}

/// 親を付け替えても、公開範囲は指定どおりに保存する。付け替えで実際の見え方が緩んでも
/// 書き換えない (→ docs/access.md「親が外れるときの公開範囲」)。
#[sqlx::test]
async fn moving_content_keeps_requested_visibility(pool: SqlitePool) {
    let staff = insert_group_under(&pool, "職員用", "authenticated", None).await;
    let link = insert_link_by(&pool, "社内リンク", "public", None, Some(staff)).await;
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app,
        "PUT",
        &format!("/api/v1/contents/{link}"),
        &cookie,
        r#"{"title":"社内リンク","url":"https://example.com","parentId":null,"visibility":"public"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(stored_visibility(&pool, link).await, "public");
}

/// multipart の差し替えでも、付け替えで公開範囲を書き換えない。
#[sqlx::test]
async fn replacing_upload_while_moving_out_keeps_requested_visibility(pool: SqlitePool) {
    let staff = insert_group_under(&pool, "職員用", "authenticated", None).await;
    let (app, _blobs_dir) = test_app_with_blobs(pool.clone()).await;
    let cookie = admin_cookie(app.clone(), &pool).await;

    let response = app
        .clone()
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[("title", "資料"), ("parentId", &staff.to_string())],
            Some(("file", "doc.txt", b"hello")),
        ))
        .await
        .expect("request should not fail");
    assert_eq!(response.status(), StatusCode::CREATED);
    let id = created_id(response).await;

    let response = app
        .oneshot(multipart_request(
            "PUT",
            &format!("/api/v1/contents/{id}/upload"),
            &cookie,
            &[("title", "資料"), ("visibility", "public")],
            None,
        ))
        .await
        .expect("request should not fail");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(stored_visibility(&pool, id).await, "public");
}

/// グループを削除しても、ルート直下へ移る子の公開範囲は書き換えない (→ docs/access.md「親が外れるときの公開範囲」)。
#[sqlx::test]
async fn deleting_group_keeps_children_visibility(pool: SqlitePool) {
    let staff = insert_group_under(&pool, "職員用", "authenticated", None).await;
    let public_child = insert_link_by(&pool, "公開の子", "public", None, Some(staff)).await;
    let chapter = insert_group_under(&pool, "第1章", "public", Some(staff)).await;
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) =
        send_empty(app, "DELETE", &format!("/api/v1/contents/{staff}"), &cookie).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    assert_eq!(stored_visibility(&pool, public_child).await, "public");
    assert_eq!(stored_visibility(&pool, chapter).await, "public");
}

/// groupを親に指定してlinkを作成すると成功し、レスポンスに`parentId`が反映される。
#[sqlx::test]
async fn content_create_with_group_parent_succeeds(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/contents",
        &cookie,
        r#"{"type":"group","title":"資料"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let group_id = extract_id(&body);

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/contents",
        &cookie,
        &format!(
            r#"{{"type":"link","parentId":{group_id},"title":"社内Wiki","url":"https://example.com"}}"#
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(
        body.contains(&format!(r#""parentId":{group_id}"#)),
        "{body}"
    );
}

/// groupの`parentId`に自分自身を指定すると422(循環参照)。
/// 親の決まりの場合分け (無い親・グループでない親・深い子孫) は `validate_parent` の単体テストで見る。
#[sqlx::test]
async fn content_update_parent_to_self_is_validation_error(pool: SqlitePool) {
    let group_id = insert_group_under(&pool, "グループA", "authenticated", None).await;
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app,
        "PUT",
        &format!("/api/v1/contents/{group_id}"),
        &cookie,
        &format!(r#"{{"parentId":{group_id},"title":"グループA"}}"#),
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
}

/// `n`個のgroupを`ids[0]`(ルート)→`ids[1]`→...→`ids[n-1]`という一本のチェーンとして
/// 作成し、idを祖先→子孫の順で返す。50階層を超えるネストでも階層の辿り方が
/// 途中で打ち切られないことを確かめるテストで、そのネストを用意するために使う。
async fn insert_group_chain(pool: &SqlitePool, n: usize) -> Vec<i64> {
    let mut ids: Vec<i64> = Vec::with_capacity(n);
    for i in 0..n {
        let parent = ids.last().copied();
        let title = format!("group-{i}");
        let id = insert_group_under(pool, &title, "authenticated", parent).await;
        ids.push(id);
    }
    ids
}

/// `GET /contents/{id}/group`は祖先パス(ルートに近い順)と直下の子一覧を返す。
/// group以外のidを指定すると404になる。
#[sqlx::test]
async fn browse_group_returns_children_and_ancestors(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let group_a = insert_group_under(&pool, "A", "authenticated", None).await;
    let group_b = insert_group_under(&pool, "B", "authenticated", Some(group_a)).await;
    let group_c = insert_group_under(&pool, "C", "authenticated", Some(group_b)).await;
    insert_link_by(&pool, "Cの子リンク", "authenticated", None, Some(group_c)).await;
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let response = send_raw(
        app.clone(),
        build_request(
            "GET",
            &format!("/api/v1/contents/{group_c}/group"),
            Some(&cookie),
            None,
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_str(&body_string(response).await).expect("valid json");
    assert_eq!(body["groupTitle"], "C");
    let ancestor_ids: Vec<i64> = body["ancestors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_i64().unwrap())
        .collect();
    // ルートに近い順: A -> B。
    assert_eq!(ancestor_ids, vec![group_a, group_b]);
    let entries = body["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["title"], "Cの子リンク");

    // group以外(link)のidを指定すると404。
    let child_link_id = entries[0]["id"].as_i64().unwrap();
    let (status, _) = send_empty(
        app,
        "GET",
        &format!("/api/v1/contents/{child_link_id}/group"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// `GET /contents/{id}/group`の祖先取得も、50階層を超えるネストで祖先パスが
/// 途中で欠落しない(ルートまで正しく辿れる)ことを確認する。
#[sqlx::test]
async fn browse_group_ancestors_are_not_truncated_beyond_fifty_levels(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    const N: usize = 60;
    let chain = insert_group_chain(&pool, N).await;
    let leaf = *chain.last().unwrap();
    let app = test_app(pool).await;
    let cookie = login(app.clone(), "alice", "correct-password").await;

    let response = send_raw(
        app,
        build_request(
            "GET",
            &format!("/api/v1/contents/{leaf}/group"),
            Some(&cookie),
            None,
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_str(&body_string(response).await).expect("valid json");
    let ancestor_ids: Vec<i64> = body["ancestors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_i64().unwrap())
        .collect();
    // leaf自身を除くN-1件全て(chain[0..N-1])がルートに近い順で揃っているはず。
    assert_eq!(ancestor_ids, chain[..N - 1]);
}

/// groupを削除すると直接の子(link/file)はルート直下(parent_id = NULL)に昇格し、
/// file子のblobは削除されない(削除されたのはgroup自身であり、reparentされた
/// file行はまだ生きているため)。
#[sqlx::test]
async fn delete_group_reparents_children_to_root(pool: SqlitePool) {
    let group_id = insert_group_under(&pool, "資料", "authenticated", None).await;
    let (app, _blobs_dir) = test_app_with_blobs(pool.clone()).await;
    let cookie = admin_cookie(app.clone(), &pool).await;

    let upload_response = app
        .clone()
        .oneshot(multipart_request(
            "POST",
            "/api/v1/contents/upload",
            &cookie,
            &[
                ("title", "配下ファイル"),
                ("parentId", &group_id.to_string()),
                ("visibility", "authenticated"),
            ],
            Some(("file", "doc.txt", b"hello group")),
        ))
        .await
        .unwrap();
    assert_eq!(upload_response.status(), StatusCode::CREATED);
    let file_id = created_id(upload_response).await;

    let (delete_status, _) = send_empty(
        app.clone(),
        "DELETE",
        &format!("/api/v1/contents/{group_id}"),
        &cookie,
    )
    .await;
    assert_eq!(delete_status, StatusCode::NO_CONTENT);

    // reparentされたfile行のparentIdがnullになっていることを確認。
    let admin_list_response = send_raw(
        app.clone(),
        build_request("GET", "/api/v1/admin/contents", Some(&cookie), None),
    )
    .await;
    let admin_body: serde_json::Value =
        serde_json::from_str(&body_string(admin_list_response).await).expect("valid json");
    let file_entry = admin_body
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"].as_i64() == Some(file_id))
        .expect("file content should still exist");
    assert!(file_entry["parentId"].is_null(), "{file_entry}");

    // blobがGCされておらず、ダウンロードが引き続き成功することを確認。
    let download_response = send_raw(
        app,
        build_request(
            "GET",
            &format!("/api/v1/contents/{file_id}/download"),
            Some(&cookie),
            None,
        ),
    )
    .await;
    assert_eq!(download_response.status(), StatusCode::OK);
    let bytes = to_bytes(download_response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&bytes[..], b"hello group");
}

/// folderコンテンツの作成はadminなら成功し、レスポンス(admin向け)には
/// canonicalize済みのpathが含まれる。入力パスをそのまま比較すると
/// macOSでは`/tmp`が`/private/tmp`に解決される等で一致しないため、
/// テスト側も同じ`canonicalize`を通した値と比較することで、実際に
/// 正規化されて保存されていること自体を検証する(単に`path`キーが
/// 存在するだけの弱いアサーションにしない)。
/// Windowsの`canonicalize`が付ける verbatim 接頭辞はAPIが落とすため、
/// 期待値側からも落としてから比べる (→ `canonical_path_as_api_returns_it`)。
#[sqlx::test]
async fn folder_content_create_by_admin_succeeds(pool: SqlitePool) {
    let dir = temp_test_dir("create-ok");
    let canonical_dir = dir.canonicalize().unwrap();

    let (app, cookie) = admin_app(&pool).await;

    let path_json = json_string(&dir.display().to_string());
    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/contents",
        &cookie,
        &format!(r#"{{"type":"folder","title":"教材","path":{path_json}}}"#),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert!(body.contains(r#""type":"folder""#), "{body}");
    assert!(body.contains(r#""title":"教材""#), "{body}");
    let created: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        created["path"].as_str().unwrap(),
        canonical_path_as_api_returns_it(&canonical_dir),
        "{body}"
    );
}

/// パスの決まりの場合分け (無いパス・ファイル・公開できるフォルダーの外) は
/// `fs::canonical_dir_within_roots` の単体テストで見る。
#[sqlx::test]
async fn folder_content_create_with_relative_path_is_validation_error(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/contents",
        &cookie,
        r#"{"type":"folder","title":"教材","path":"relative/path"}"#,
    )
    .await;

    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
}

/// ディレクトリ一覧APIは編集者にも開いている (→ docs/access.md「ロールと操作」)。
#[sqlx::test]
async fn fs_dirs_is_open_to_regular_users(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let app = test_app(pool).await;

    let cookie = login(app.clone(), "alice", "correct-password").await;
    let (status, _) = send_empty(app, "GET", "/api/v1/admin/fs/dirs", &cookie).await;
    assert_eq!(status, StatusCode::OK);
}

/// `path` を省略すると、登録済みの「公開できるフォルダー」が並ぶ (→ docs/folders.md「公開できるフォルダー」)。
/// ドライブの一覧ではない。
#[sqlx::test]
async fn fs_dirs_without_path_returns_the_shared_folders(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let response = send_raw(
        app,
        build_request("GET", "/api/v1/admin/fs/dirs", Some(&cookie), None),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
    assert_eq!(body["path"], "");
    assert!(body["parent"].is_null(), "{body}");
    assert_eq!(body["selectable"], false);
    let entries = body["entries"].as_array().expect("entriesが配列でない");
    let expected = canonical_path_as_api_returns_it(
        &std::fs::canonicalize(content_root()).expect("canonicalize できなかった"),
    );
    assert_eq!(entries.len(), 1, "{body}");
    assert_eq!(entries[0]["path"], expected, "{body}");
    // 名前は登録した名前 (→ `register_root` はパスと同じにしている)。
    assert_eq!(entries[0]["name"], expected, "{body}");
    // 登録済みのフォルダーそのものは、コンテンツの登録先に選べる。
    assert_eq!(entries[0]["selectable"], true, "{body}");
}

/// 「公開できるフォルダー」の外は、一覧そのものを見せない。
#[sqlx::test]
async fn fs_dirs_rejects_a_path_outside_the_shared_folders(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, _) = send_empty(app, "GET", "/api/v1/admin/fs/dirs?path=/", &cookie).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

/// ディレクトリだけが返り、ファイル・ドット始まりは含まれない。
/// 通常のディレクトリは `selectable` になる。
#[sqlx::test]
async fn fs_dirs_returns_only_selectable_directories(pool: SqlitePool) {
    let dir = temp_test_dir("fs-dirs");
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::create_dir_all(dir.join(".hidden")).unwrap();
    std::fs::write(dir.join("file.txt"), b"hello").unwrap();

    let (app, cookie) = admin_app(&pool).await;

    let response = send_raw(
        app,
        build_request(
            "GET",
            &format!("/api/v1/admin/fs/dirs?path={}", dir.display()),
            Some(&cookie),
            None,
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
    assert_eq!(body["selectable"], true);
    let entries = body["entries"].as_array().expect("entriesが配列でない");
    assert_eq!(entries.len(), 1, "{body}");
    assert_eq!(entries[0]["name"], "sub");
    assert_eq!(entries[0]["selectable"], true);
}

/// 件数APIは再帰的に数え、`extensions` で絞り込める。ドット始まりは数えない。
#[sqlx::test]
async fn fs_count_counts_recursively_and_filters_by_extension(pool: SqlitePool) {
    let dir = temp_test_dir("fs-count");
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("a.mp3"), b"").unwrap();
    std::fs::write(dir.join(".hidden.mp3"), b"").unwrap();
    std::fs::write(dir.join("sub").join("b.pdf"), b"").unwrap();

    let (app, cookie) = admin_app(&pool).await;
    let path = dir.display().to_string();

    let (status, body) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/admin/fs/count?path={path}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#""count":2"#), "{body}");
    assert!(body.contains(r#""truncated":false"#), "{body}");

    let (status, body) = send_empty(
        app,
        "GET",
        &format!("/api/v1/admin/fs/count?path={path}&extensions=.MP3"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#""count":1"#), "{body}");
}

/// 件数APIも「公開できるフォルダー」の外を拒否する
/// (件数を出せる = 登録できる、と読めてしまうのを防ぐ)。
#[sqlx::test]
async fn fs_count_rejects_a_directory_outside_the_shared_folders(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, _) = send_empty(app, "GET", "/api/v1/admin/fs/count?path=/", &cookie).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

/// 管理画面専用一覧(`GET /admin/contents`)にはpathが含まれるが、一般公開の
/// `GET /contents`には含まれない(匿名を含む一般閲覧者にファイルシステムのレイアウトを
/// 漏らさない)。一般ユーザーは `/admin/contents` を見られる (→ docs/access.md「管理画面の一覧が `user` に見えること」)。
#[sqlx::test]
async fn admin_contents_list_includes_path_and_is_open_to_regular_users(pool: SqlitePool) {
    insert_user(&pool, "alice", "correct-password").await;
    let dir = temp_test_dir("admin-list");

    let app = test_app(pool.clone()).await;
    let admin_cookie = admin_cookie(app.clone(), &pool).await;

    let path_json = json_string(&dir.display().to_string());
    let _ = send_json(
        app.clone(),
        "POST",
        "/api/v1/contents",
        &admin_cookie,
        &format!(r#"{{"type":"folder","title":"教材","path":{path_json}}}"#),
    )
    .await;

    let (admin_list_status, admin_list_body) =
        send_empty(app.clone(), "GET", "/api/v1/admin/contents", &admin_cookie).await;
    assert_eq!(admin_list_status, StatusCode::OK);
    assert!(admin_list_body.contains(r#""path":"#), "{admin_list_body}");

    let user_cookie = login(app.clone(), "alice", "correct-password").await;
    let (public_list_status, public_list_body) =
        send_empty(app.clone(), "GET", "/api/v1/contents", &user_cookie).await;
    assert_eq!(public_list_status, StatusCode::OK);
    assert!(!public_list_body.contains("\"path\""), "{public_list_body}");
    assert!(
        public_list_body.contains(r#""type":"folder""#),
        "{public_list_body}"
    );

    // 管理画面の一覧は `user` にも開放されている。`path` が見えるのは要件が許容している
    // (登録できるのは「公開できるフォルダー」の中だけなので、見せたくないパスは登録されない)。
    let (user_admin_list_status, user_admin_list_body) =
        send_empty(app, "GET", "/api/v1/admin/contents", &user_cookie).await;
    assert_eq!(user_admin_list_status, StatusCode::OK);
    assert!(
        user_admin_list_body.contains(r#""path":"#),
        "{user_admin_list_body}"
    );
}
