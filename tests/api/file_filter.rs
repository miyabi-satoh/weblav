//! フォルダーで見せるものの絞り込み (→ docs/folders.md「見せるものの絞り込み」)。
//! パターンの決まりは `file_filter` の単体テストで見る。ここは、その決まりが各入口につながっていること。

use super::*;

async fn set_patterns(pool: &SqlitePool, id: i64, include: Option<&str>, exclude: Option<&str>) {
    sqlx::query!(
        "UPDATE contents SET include_patterns = ?, exclude_patterns = ? WHERE id = ?",
        include,
        exclude,
        id
    )
    .execute(pool)
    .await
    .expect("設定を変えられなかった");
}

/// 外れるファイルは、見る人にも編集できる人にも、一覧・ダウンロード・検索のどれにも出ない。
/// 対象だけを書いたとき、当てはまるファイルの無いサブフォルダーは並ぶ。
#[sqlx::test]
async fn patterns_reach_the_folder_list_download_and_search(pool: SqlitePool) {
    let dir = temp_test_dir("folder-patterns");
    std::fs::write(dir.join("a.pdf"), b"a").expect("ファイルを作れなかった");
    std::fs::write(dir.join("b.txt"), b"b").expect("ファイルを作れなかった");
    std::fs::create_dir_all(dir.join("notes")).expect("フォルダーを作れなかった");
    std::fs::write(dir.join("notes").join("c.txt"), b"c").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let id = insert_folder_by(&pool, "資料", &*dir, "public", None, None).await;
    set_patterns(&pool, id, Some("*.pdf"), None).await;

    let browse = format!("/api/v1/contents/{id}/browse?path=");
    let (status, body) = send_anon(app.clone(), "GET", &browse).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(entry_names(&body), ["notes", "a.pdf"], "{body}");
    let (_, body) = send_empty(app.clone(), "GET", &browse, &cookie).await;
    assert_eq!(entry_names(&body), ["notes", "a.pdf"], "{body}");
    let (status, body) = send_anon(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/browse?path=notes"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(entry_names(&body).is_empty(), "{body}");

    let download = |path: &str| format!("/api/v1/contents/{id}/download?path={path}");
    let (status, _) = send_anon(app.clone(), "GET", &download("a.pdf")).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send_anon(app.clone(), "GET", &download("b.txt")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send_empty(app.clone(), "GET", &download("notes/c.txt"), &cookie).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // 綴りの大文字小文字を変えても、実体のパスで突き合わせるので外れる (大文字小文字を区別しないディスクだけ)。
    if cfg!(target_os = "macos") {
        set_patterns(&pool, id, None, Some("/b.txt")).await;
        let (status, _) = send_anon(app.clone(), "GET", &download("B.TXT")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        set_patterns(&pool, id, Some("*.pdf"), None).await;
    }

    let (_, body) = send_anon(app.clone(), "GET", "/api/v1/search?q=c.txt").await;
    assert!(!body.contains("c.txt"), "{body}");
    // 外していないものは当たる (検索がフォルダーの中を見ていることの確かめ)。
    let (_, body) = send_anon(app, "GET", "/api/v1/search?q=a.pdf").await;
    assert!(body.contains("a.pdf"), "{body}");
}

/// 除外したフォルダーは開けない。登録先そのものは、見せるものが無くても開けて、空に見える。
/// 正しくないパターンは保存しない。
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
    assert_eq!(entry_names(&body), ["b.txt"], "{body}");
    let (status, _) = send_anon(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/browse?path=drafts"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    set_patterns(&pool, id, None, Some("*")).await;
    let (status, body) = send_anon(app, "GET", &browse).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(entry_names(&body).is_empty(), "{body}");
}

/// ライブラリでは、パターンを変えると再スキャンを待たずに、一覧・配信・検索に効く。
/// 管理の一覧と管理の配信にも当たる。
#[sqlx::test]
async fn pattern_changes_reach_the_library_without_a_rescan(pool: SqlitePool) {
    let dir = temp_test_dir("library-patterns");
    std::fs::write(dir.join("keep.mp3"), b"").expect("ファイルを作れなかった");
    std::fs::write(dir.join("hidden.mp3"), b"").expect("ファイルを作れなかった");
    let (app, cookie, id) = setup_archive(&pool, &dir, None).await;
    let items = manage_items(app.clone(), &cookie, id).await;
    assert_eq!(items.len(), 2);
    let hidden_id = items
        .iter()
        .find(|item| item["fileName"] == "hidden.mp3")
        .and_then(|item| item["id"].as_i64())
        .expect("hidden.mp3 が索引に無かった");

    set_patterns(&pool, id, None, Some("hidden.*")).await;

    let (status, body) = view_archive(app.clone(), "", id, &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let view = parse_json(&body);
    assert_eq!(view["total"], 1, "{body}");
    assert_eq!(view["items"][0]["fileName"], "keep.mp3", "{body}");
    let items = manage_items(app.clone(), &cookie, id).await;
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["fileName"], "keep.mp3", "{items:?}");

    let (status, _) = send_anon(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/items/{hidden_id}/download"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/items/{hidden_id}/manage-download"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, body) = send_anon(app, "GET", "/api/v1/search?q=hidden").await;
    assert!(!body.contains("hidden.mp3"), "{body}");
}

/// スキャンはパターンによらず全部を索引するので、外れた行が、索引を読むどの口からも出ない。
/// 口を足したら、ここにも足す。
#[sqlx::test]
async fn excluded_rows_in_the_index_stay_out_of_every_reader(pool: SqlitePool) {
    let dir = temp_test_dir("library-leaks");
    std::fs::create_dir_all(dir.join("keep")).expect("フォルダーを作れなかった");
    std::fs::create_dir_all(dir.join("himitsu")).expect("フォルダーを作れなかった");
    std::fs::write(dir.join("keep").join("hyoji.pdf"), b"a").expect("ファイルを作れなかった");
    std::fs::write(dir.join("himitsu").join("himitsu.pdf"), b"b").expect("ファイルを作れなかった");
    std::fs::write(
        dir.join("himitsu.links.toml"),
        b"[[links]]\nurl = \"https://example.com/himitsu\"\n",
    )
    .expect("ファイルを作れなかった");
    let (app, cookie, id) = setup_archive(&pool, &dir, None).await;
    set_patterns(&pool, id, None, Some("himitsu/\nhimitsu.*")).await;
    let (status, body) = rescan(app.clone(), &cookie, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // 結果の件数は、見出しの件数と同じく見えるものだけを数える。
    assert_eq!(parse_json(&body)["total"], 1, "{body}");
    let (indexed_id, indexed_rel) = sqlx::query_as::<_, (i64, String)>(
        "SELECT id, rel_path FROM archive_items WHERE archive_id = ? AND rel_path LIKE 'himitsu/%'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("除外したフォルダーの中も索引に入っているはず");
    assert_eq!(indexed_rel, "himitsu/himitsu.pdf");

    let axis_id = create_axis(
        app.clone(),
        &cookie,
        id,
        r#"{"name":"階層","source":"dirLevel","dirLevel":1,"position":0}"#,
    )
    .await;

    // 一覧・分類の手がかり・AI に渡すパスの例。どれにも外れたものの名前が出ない。
    for reader in [
        format!("/api/v1/contents/{id}/archive"),
        format!("/api/v1/contents/{id}/manage-archive"),
        format!("/api/v1/contents/{id}/path-samples"),
        format!("/api/v1/contents/{id}/dir-levels"),
        format!("/api/v1/contents/{id}/filename-words"),
        format!("/api/v1/contents/{id}/axes/{axis_id}/values"),
        "/api/v1/search?q=himitsu".to_string(),
    ] {
        let (status, body) = send_empty(app.clone(), "GET", &reader, &cookie).await;
        assert_eq!(status, StatusCode::OK, "{reader}: {body}");
        assert!(!body.contains("himitsu"), "{reader}: {body}");
    }
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/v1/contents/{id}/axes/{axis_id}/values/preview"),
        &cookie,
        r#"{"values":[]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse_json(&body)["itemCount"], 1, "{body}");
    assert!(!body.contains("himitsu"), "{body}");
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/v1/contents/{id}/axes/import/preview"),
        &cookie,
        r#"{"axes":[]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains("himitsu"), "{body}");

    // 配信は、閲覧の口も管理の口も断る。
    for opener in [
        format!("/api/v1/contents/{id}/items/{indexed_id}/download"),
        format!("/api/v1/contents/{id}/items/{indexed_id}/thumbnail"),
        format!("/api/v1/contents/{id}/items/{indexed_id}/manage-download"),
        format!("/api/v1/contents/{id}/items/{indexed_id}/manage-thumbnail"),
        format!("/api/v1/contents/{id}/archive/download?path=himitsu/himitsu.pdf"),
        format!("/api/v1/contents/{id}/archive/manage-download?path=himitsu/himitsu.pdf"),
    ] {
        let (status, body) = send_empty(app.clone(), "GET", &opener, &cookie).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{opener}: {body}");
    }

    // 除外を外せば、再スキャンなしで見えるようになる。
    set_patterns(&pool, id, None, None).await;
    assert_eq!(manage_items(app, &cookie, id).await.len(), 3);
}

/// 対象外のタブの口は、外れるファイルのパスを返し、語はパスに当たる。書き換えられない人には見せない。
#[sqlx::test]
async fn excluded_items_list_paths_for_writers_only(pool: SqlitePool) {
    let dir = temp_test_dir("library-excluded");
    std::fs::create_dir_all(dir.join("drafts")).expect("フォルダーを作れなかった");
    std::fs::write(dir.join("a.pdf"), b"a").expect("ファイルを作れなかった");
    std::fs::write(dir.join("drafts").join("b.pdf"), b"b").expect("ファイルを作れなかった");
    std::fs::write(dir.join("drafts").join("c.pdf"), b"c").expect("ファイルを作れなかった");
    std::fs::write(dir.join(".dot"), b"d").expect("ファイルを作れなかった");
    let (app, cookie, id) = setup_archive(&pool, &dir, None).await;
    set_patterns(&pool, id, None, Some("drafts/")).await;

    let uri = format!("/api/v1/contents/{id}/excluded-items");
    let (status, body) = send_empty(app.clone(), "GET", &uri, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let listed = parse_json(&body);
    assert_eq!(listed["total"], 2, "{body}");
    assert_eq!(listed["items"][0]["relPath"], "drafts/b.pdf", "{body}");
    assert_eq!(listed["items"][1]["relPath"], "drafts/c.pdf", "{body}");
    assert_eq!(listed["items"].as_array().map(Vec::len), Some(2), "{body}");

    // 語はフォルダーの名前にも当たる (パスに当てるので)。
    let (_, body) = send_empty(app.clone(), "GET", &format!("{uri}?filter=c.pdf"), &cookie).await;
    let filtered = parse_json(&body);
    assert_eq!(
        (&filtered["total"], &filtered["totalAll"]),
        (&1.into(), &2.into()),
        "{body}"
    );
    let (_, body) = send_empty(app.clone(), "GET", &format!("{uri}?filter=drafts"), &cookie).await;
    assert_eq!(parse_json(&body)["total"], 2, "{body}");

    // 続きは、作った人の結果だけを引ける。行を見せないので、閲覧の口の結果としても引けない。
    let snapshot = listed["snapshot"].as_str().expect("結果の名前があるはず");
    let (status, body) = send_empty(
        app.clone(),
        "GET",
        &format!("{uri}/page?snapshot={snapshot}&offset=1"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        parse_json(&body)["items"][0]["relPath"],
        "drafts/c.pdf",
        "{body}"
    );
    let (status, body) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/manage-archive/page?snapshot={snapshot}&offset=0"),
        &cookie,
    )
    .await;
    assert_error(status, &body, StatusCode::CONFLICT, "conflict");

    insert_user(&pool, "taro", "password").await;
    let taro = login(app.clone(), "taro", "password").await;
    let (status, body) = send_empty(app, "GET", &uri, &taro).await;
    assert_error(status, &body, StatusCode::FORBIDDEN, "forbidden");
}

/// ライブラリの増減は索引から数える。ディスクを読まないので、消えたファイルも次に索引が直るまでは数える。
#[sqlx::test]
async fn visibility_preview_of_a_library_counts_from_the_index(pool: SqlitePool) {
    let dir = temp_test_dir("library-preview");
    std::fs::write(dir.join("a.pdf"), b"a").expect("ファイルを作れなかった");
    std::fs::write(dir.join("b.pdf"), b"b").expect("ファイルを作れなかった");
    let (app, cookie, id) = setup_archive(&pool, &dir, None).await;
    std::fs::remove_file(dir.join("a.pdf")).expect("ファイルを消せなかった");

    let (status, body) = send_json(
        app,
        "POST",
        &format!("/api/v1/contents/{id}/visibility-preview"),
        &cookie,
        r#"{"excludePatterns":"a.*"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        parse_json(&body),
        serde_json::json!({"gained": 0, "lost": 1, "truncated": false}),
        "{body}"
    );
}

/// 除外した場所が読めなくても再スキャンは止まらず、その中の行を消さない。
/// 見せる場所が読めなければ、今までどおり断る。
#[cfg(unix)]
#[sqlx::test]
async fn an_unreadable_excluded_folder_does_not_block_a_rescan(pool: SqlitePool) {
    let dir = temp_test_dir("library-unreadable");
    std::fs::create_dir_all(dir.join("recycle").join("sub")).expect("フォルダーを作れなかった");
    std::fs::write(dir.join("recycle").join("old.pdf"), b"a").expect("ファイルを作れなかった");
    std::fs::write(dir.join("recycle").join("sub").join("x.pdf"), b"a")
        .expect("ファイルを作れなかった");
    std::fs::write(dir.join("keep.pdf"), b"b").expect("ファイルを作れなかった");
    let (app, cookie, id) = setup_archive(&pool, &dir, None).await;
    let rows = || async {
        sqlx::query_scalar::<_, String>(
            "SELECT rel_path FROM archive_items WHERE archive_id = ? ORDER BY rel_path",
        )
        .bind(id)
        .fetch_all(&pool)
        .await
        .expect("索引を読めなかった")
    };
    assert_eq!(
        rows().await,
        ["keep.pdf", "recycle/old.pdf", "recycle/sub/x.pdf"]
    );
    let _locked = test_support::Unreadable::new(dir.join("recycle"));

    set_patterns(&pool, id, None, Some("recycle/")).await;
    // 一覧を開いたときの読み直しも、読めないだけのフォルダーを消えたと見なさない。
    manage_items(app.clone(), &cookie, id).await;
    assert_eq!(
        rows().await,
        ["keep.pdf", "recycle/old.pdf", "recycle/sub/x.pdf"]
    );

    // 読み直しが裏で排他を持たないよう、足すのは一覧を開いた後にする。
    std::fs::write(dir.join("new.pdf"), b"c").expect("ファイルを作れなかった");
    let (status, body) = rescan(app.clone(), &cookie, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse_json(&body)["removed"], 0, "{body}");
    assert_eq!(
        rows().await,
        [
            "keep.pdf",
            "new.pdf",
            "recycle/old.pdf",
            "recycle/sub/x.pdf"
        ],
        "読めなかった場所の行を残し、ほかは同期するはず"
    );

    set_patterns(&pool, id, None, None).await;
    let (status, body) = rescan(app, &cookie, id).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

/// 保存の前に、見えるファイルの増減を数えられる。正しくないパターンは 422。
#[sqlx::test]
async fn visibility_preview_counts_gained_and_lost_files(pool: SqlitePool) {
    let dir = temp_test_dir("folder-preview");
    std::fs::write(dir.join("a.pdf"), b"a").expect("ファイルを作れなかった");
    std::fs::write(dir.join("b.txt"), b"b").expect("ファイルを作れなかった");
    let (app, cookie) = admin_app(&pool).await;
    let id = insert_folder_by(&pool, "資料", &*dir, "public", None, None).await;
    set_patterns(&pool, id, Some("*.pdf"), None).await;
    let uri = format!("/api/v1/contents/{id}/visibility-preview");

    let (status, body) = send_json(
        app.clone(),
        "POST",
        &uri,
        &cookie,
        r#"{"includePatterns":"*.txt"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        parse_json(&body),
        serde_json::json!({"gained": 1, "lost": 1, "truncated": false}),
        "{body}"
    );

    let (status, body) = send_json(app, "POST", &uri, &cookie, r#"{"excludePatterns":"[a"}"#).await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
}
