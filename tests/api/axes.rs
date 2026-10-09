//! アーカイブの軸・値の辞書 (→ docs/archive.md「軸の定義」・「軸の値の辞書と導出」・「表示タイトル」)

use super::*;

#[sqlx::test]
async fn axis_crud_round_trip_as_admin(pool: SqlitePool) {
    let dir = temp_test_dir("axis-crud");

    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/v1/contents/{id}/axes"),
        &cookie,
        r#"{"name":"科目","source":"dirLevel","dirLevel":1,"position":0}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let axis: serde_json::Value =
        serde_json::from_str(&body).expect("軸の作成レスポンスを読めなかった");
    assert_eq!(axis["name"], "科目", "{body}");
    assert_eq!(axis["source"], "dirLevel", "{body}");
    assert_eq!(axis["dirLevel"], 1, "{body}");
    // 省略したら絞り込みに出す (→ docs/archive.md「軸の定義」)。
    assert_eq!(axis["filterable"], true, "{body}");
    let axis_id = extract_id(&body);

    // JSON表現(camelCase)とDB上の実値(snake_case)がずれていないことを確認する。
    // 1cの導出や既存テストの手書きSQL(`deleting_archive_cascades_to_items_axes_and_values`)は
    // このDB表現に依存する。
    let stored_source =
        sqlx::query_scalar!("SELECT source FROM archive_axes WHERE id = ?", axis_id)
            .fetch_one(&pool)
            .await
            .expect("sourceを読めなかった");
    assert_eq!(stored_source, "dir_level");

    let (status, body) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/axes"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("科目"), "{body}");

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}/axes/{axis_id}"),
        &cookie,
        r#"{"name":"教科","source":"dirLevel","dirLevel":1,"filterable":false,"position":1}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let updated: serde_json::Value =
        serde_json::from_str(&body).expect("軸の更新レスポンスを読めなかった");
    assert_eq!(updated["name"], "教科", "{body}");
    assert_eq!(updated["filterable"], false, "{body}");
    assert_eq!(updated["position"], 1, "{body}");

    let (status, body) = send_empty(
        app.clone(),
        "DELETE",
        &format!("/api/v1/contents/{id}/axes/{axis_id}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (status, body) =
        send_empty(app, "GET", &format!("/api/v1/contents/{id}/axes"), &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, "[]", "{body}");
}

/// 他人のアーカイブの軸は、一覧(GET)は見えるが追加(POST)は403。自分のアーカイブには追加できる
/// (→ docs/access.md「ロールと操作」)。
#[sqlx::test]
async fn axis_create_by_regular_user_is_limited_to_own_archives(pool: SqlitePool) {
    insert_user(&pool, "bob", "correct-password").await;
    let dir = temp_test_dir("axis-forbidden");

    let app = test_app(pool.clone()).await;
    let admin_cookie = admin_cookie(app.clone(), &pool).await;
    let id = create_archive(app.clone(), &admin_cookie, &dir, None).await;

    let user_cookie = login(app.clone(), "bob", "correct-password").await;
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/v1/contents/{id}/axes"),
        &user_cookie,
        r#"{"name":"科目","source":"filenameWord","position":0}"#,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let (status, body) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/axes"),
        &user_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let own_dir = temp_test_dir("axis-own-archive");
    let own_id = create_archive(app.clone(), &user_cookie, &own_dir, None).await;
    create_axis(
        app,
        &user_cookie,
        own_id,
        r#"{"name":"科目","source":"filenameWord","position":0}"#,
    )
    .await;
}

/// 軸名と抽出元の場合分けは `validate_axis_name`・`validate_axis_source` の単体テストで見る。
/// ここは、作成の入口につながり、内訳 (`detail`) が応答に載ることだけ。
#[sqlx::test]
async fn axis_create_validates_name_and_source(pool: SqlitePool) {
    let dir = temp_test_dir("axis-validate");

    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;

    let (status, body) = send_json(
        app,
        "POST",
        &format!("/api/v1/contents/{id}/axes"),
        &cookie,
        r#"{"name":"{科目}","source":"filenameWord","position":0}"#,
    )
    .await;

    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
    assert!(body.contains(r#""kind":"axisNameHasBraces""#), "{body}");
}

/// 軸 API は、入力の検証 (422) より存在の判定 (404) を先に行う。
/// コンテンツの更新 (`update_content`) と順を揃える。
#[sqlx::test]
async fn axis_endpoints_report_missing_archive_before_invalid_input(pool: SqlitePool) {
    let dir = temp_test_dir("axis-404-before-422");

    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;

    let invalid = r#"{"name":"","source":"filenameWord","position":0}"#;
    for (method, uri) in [
        // アーカイブが無い。
        ("POST", "/api/v1/contents/999999/axes".to_string()),
        ("PUT", "/api/v1/contents/999999/axes/1".to_string()),
        // アーカイブはあるが、軸が無い。
        ("PUT", format!("/api/v1/contents/{id}/axes/999999")),
    ] {
        let (status, body) = send_json(app.clone(), method, &uri, &cookie, invalid).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}: {body}");
    }
}

/// 軸名はアーカイブ内で一意 (→ docs/archive.md「軸の定義」)。別アーカイブでは同名を許す。
#[sqlx::test]
async fn axis_name_must_be_unique_within_the_same_archive(pool: SqlitePool) {
    let dir_a = temp_test_dir("axis-unique-a");
    let dir_b = temp_test_dir("axis-unique-b");
    std::fs::create_dir_all(&dir_a).expect("登録先を作れなかった");
    std::fs::create_dir_all(&dir_b).expect("登録先を作れなかった");

    let (app, cookie) = admin_app(&pool).await;
    let id_a = create_archive(app.clone(), &cookie, &dir_a, None).await;
    let id_b = create_archive(app.clone(), &cookie, &dir_b, None).await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/v1/contents/{id_a}/axes"),
        &cookie,
        r#"{"name":"科目","source":"filenameWord","position":0}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/v1/contents/{id_a}/axes"),
        &cookie,
        r#"{"name":"科目","source":"filenameWord","position":1}"#,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // 別アーカイブでは同名を許す。
    let (status, body) = send_json(
        app,
        "POST",
        &format!("/api/v1/contents/{id_b}/axes"),
        &cookie,
        r#"{"name":"科目","source":"filenameWord","position":0}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// 所属条件 (`WHERE id = ? AND archive_id = ?`) を全エンドポイントで確認する
/// (→ docs/archive.md「エンドポイント一覧」)。別アーカイブの軸idを渡すと404。
#[sqlx::test]
async fn axis_endpoints_404_for_axis_belonging_to_another_archive(pool: SqlitePool) {
    let dir_a = temp_test_dir("axis-404-a");
    let dir_b = temp_test_dir("axis-404-b");
    std::fs::create_dir_all(&dir_a).expect("登録先を作れなかった");
    std::fs::create_dir_all(&dir_b).expect("登録先を作れなかった");

    let (app, cookie) = admin_app(&pool).await;
    let id_a = create_archive(app.clone(), &cookie, &dir_a, None).await;
    let id_b = create_archive(app.clone(), &cookie, &dir_b, None).await;

    let axis_id = create_axis(
        app.clone(),
        &cookie,
        id_a,
        r#"{"name":"科目","source":"filenameWord","position":0}"#,
    )
    .await;

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id_b}/axes/{axis_id}"),
        &cookie,
        r#"{"name":"教科","source":"filenameWord","position":0}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = send_empty(
        app.clone(),
        "DELETE",
        &format!("/api/v1/contents/{id_b}/axes/{axis_id}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id_b}/axes/{axis_id}/values"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id_b}/axes/{axis_id}/values"),
        &cookie,
        r#"{"values":[]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // 存在しないアーカイブid・アーカイブでないコンテンツidも404。
    let (status, body) =
        send_empty(app.clone(), "GET", "/api/v1/contents/999999/axes", &cookie).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/contents",
        &cookie,
        r#"{"type":"link","title":"外部","url":"https://example.com"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let link_id = extract_id(&body);

    let (status, body) = send_empty(
        app,
        "GET",
        &format!("/api/v1/contents/{link_id}/axes"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// 軸のリネームは同一トランザクションで `title_template` を書き換える (→ docs/archive.md「表示タイトル」)。
#[sqlx::test]
async fn axis_rename_rewrites_title_template(pool: SqlitePool) {
    let dir = temp_test_dir("axis-rename-template");

    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;

    let axis_id = create_axis(
        app.clone(),
        &cookie,
        id,
        r#"{"name":"科目","source":"filenameWord","position":0}"#,
    )
    .await;

    let path_json = json_string(&dir.display().to_string());
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}"),
        &cookie,
        &format!(
            r#"{{"title":"英検 過去問","path":{path_json},"titleTemplate":"{{科目}}（過去問）"}}"#
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}/axes/{axis_id}"),
        &cookie,
        r#"{"name":"教科","source":"filenameWord","position":0}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let title_template: Option<String> =
        sqlx::query_scalar!("SELECT title_template FROM contents WHERE id = ?", id)
            .fetch_one(&pool)
            .await
            .expect("title_templateを読めなかった");
    assert_eq!(title_template.as_deref(), Some("{教科}（過去問）"));
}

/// テンプレートで参照中の軸は削除できない (→ docs/archive.md「表示タイトル」の「保存時に全プレースホルダーが実在すること」を維持するため)。
#[sqlx::test]
async fn axis_delete_is_rejected_while_referenced_by_title_template(pool: SqlitePool) {
    let dir = temp_test_dir("axis-delete-referenced");

    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;

    let axis_id = create_axis(
        app.clone(),
        &cookie,
        id,
        r#"{"name":"科目","source":"filenameWord","position":0}"#,
    )
    .await;

    let path_json = json_string(&dir.display().to_string());
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}"),
        &cookie,
        &format!(r#"{{"title":"英検 過去問","path":{path_json},"titleTemplate":"{{科目}}"}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send_empty(
        app.clone(),
        "DELETE",
        &format!("/api/v1/contents/{id}/axes/{axis_id}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // テンプレートから参照を外せば削除できる。
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}"),
        &cookie,
        &format!(r#"{{"title":"英検 過去問","path":{path_json},"titleTemplate":null}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send_empty(
        app,
        "DELETE",
        &format!("/api/v1/contents/{id}/axes/{axis_id}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
}

/// 保存時に、テンプレート中のプレースホルダーが実在する軸名であることを検証する
/// (→ docs/archive.md「表示タイトル」)。形の誤りなどの場合分けは `check_title_template` の単体テストで見る。
#[sqlx::test]
async fn title_template_validation_on_save(pool: SqlitePool) {
    let dir = temp_test_dir("title-template-validation");

    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;
    let path_json = json_string(&dir.display().to_string());

    // 軸が1つも無い状態では、どんなプレースホルダーも未定義。
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}"),
        &cookie,
        &format!(r#"{{"title":"英検 過去問","path":{path_json},"titleTemplate":"{{科目}}"}}"#),
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
    assert!(
        body.contains(r#""kind":"templateUnknownAxis","name":"科目""#),
        "{body}"
    );

    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/v1/contents/{id}/axes"),
        &cookie,
        r#"{"name":"科目","source":"filenameWord","position":0}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, body) = send_json(
        app,
        "PUT",
        &format!("/api/v1/contents/{id}"),
        &cookie,
        &format!(r#"{{"title":"英検 過去問","path":{path_json},"titleTemplate":"{{科目}}"}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// `dir_level` 軸の値の辞書は、現在のアイテムに現れる生の値と辞書を突き合わせて返す
/// (→ docs/archive.md「軸の値の辞書と導出」)。辞書に無い値は生の値のまま既定順(昇順)で末尾に並ぶ。
#[sqlx::test]
async fn axis_values_dir_level_merges_dictionary_with_current_items(pool: SqlitePool) {
    let dir = temp_test_dir("axis-values-dir-level");
    std::fs::create_dir_all(dir.join("2024")).expect("登録先を作れなかった");
    std::fs::create_dir_all(dir.join("2023")).expect("登録先を作れなかった");
    std::fs::write(dir.join("2024").join("listening.mp3"), b"").expect("ファイルを作れなかった");
    std::fs::write(dir.join("2023").join("listening.mp3"), b"").expect("ファイルを作れなかった");

    let (app, cookie, id) = setup_archive(&pool, &dir, None).await;

    let axis_id = create_axis(
        app.clone(),
        &cookie,
        id,
        r#"{"name":"年度","source":"dirLevel","dirLevel":1,"position":0}"#,
    )
    .await;

    put_axis_values(
        app.clone(),
        &cookie,
        id,
        axis_id,
        r#"{"values":[{"rawValue":"2024","displayName":"2024年度"}]}"#,
    )
    .await;

    let (status, body) = send_empty(
        app,
        "GET",
        &format!("/api/v1/contents/{id}/axes/{axis_id}/values"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let values: serde_json::Value = serde_json::from_str(&body).expect("値の辞書を読めなかった");
    assert_eq!(
        values,
        serde_json::json!([
            {"rawValue": "2024", "displayName": "2024年度", "matchPosition": "anywhere"},
            {"rawValue": "2023", "displayName": null, "matchPosition": "anywhere"},
        ]),
        "{body}"
    );
}

/// 値が無くても表示タイトルを組み立てるかを、軸ごとに保存して返す (→ docs/archive.md「表示タイトル」)。
#[sqlx::test]
async fn axis_optional_in_title_round_trip(pool: SqlitePool) {
    let dir = temp_test_dir("axis-optional-in-title");

    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;
    let axes_url = format!("/api/v1/contents/{id}/axes");

    // 省略したら、値が無ければ組み立てない。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &axes_url,
        &cookie,
        r#"{"name":"科目","source":"filenameWord","position":0}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let axis: serde_json::Value = serde_json::from_str(&body).expect("軸を読めなかった");
    assert_eq!(axis["optionalInTitle"], false);

    let axis_id = create_axis(
        app.clone(),
        &cookie,
        id,
        r#"{"name":"表紙","source":"filenameWord","optionalInTitle":true,"position":1}"#,
    )
    .await;

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("{axes_url}/{axis_id}"),
        &cookie,
        r#"{"name":"課程","source":"filenameWord","optionalInTitle":true,"position":1}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send_empty(app, "GET", &axes_url, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let axes: serde_json::Value = serde_json::from_str(&body).expect("軸の一覧を読めなかった");
    assert_eq!(axes[1]["name"], "課程");
    assert_eq!(axes[1]["optionalInTitle"], true);
}

/// 照合する位置は照合語の行ごとに保存して返す。省略した行はどこでも照合する。
/// フォルダーの階層の軸の行は照合する位置を持てない (→ docs/archive.md「軸の定義」)。
#[sqlx::test]
async fn axis_values_match_position_round_trip(pool: SqlitePool) {
    let dir = temp_test_dir("axis-values-match-position");

    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;
    let word_axis_id = create_axis(
        app.clone(),
        &cookie,
        id,
        r#"{"name":"課程","source":"filenameWord","position":0}"#,
    )
    .await;
    let dir_axis_id = create_axis(
        app.clone(),
        &cookie,
        id,
        r#"{"name":"年度","source":"dirLevel","dirLevel":1,"position":1}"#,
    )
    .await;
    let values_url = |axis_id: i64| format!("/api/v1/contents/{id}/axes/{axis_id}/values");

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &values_url(word_axis_id),
        &cookie,
        r#"{"values":[
            {"rawValue":"kyuu","displayName":"旧","matchPosition":"wordStart"},
            {"rawValue":"hyoshi","matchPosition":"end"},
            {"rawValue":"kaisetsu"}
        ]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send_empty(app.clone(), "GET", &values_url(word_axis_id), &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        parse_json(&body),
        serde_json::json!([
            {"rawValue": "kyuu", "displayName": "旧", "matchPosition": "wordStart"},
            {"rawValue": "hyoshi", "displayName": null, "matchPosition": "end"},
            {"rawValue": "kaisetsu", "displayName": null, "matchPosition": "anywhere"},
        ]),
        "{body}"
    );

    let (status, body) = send_json(
        app,
        "PUT",
        &values_url(dir_axis_id),
        &cookie,
        r#"{"values":[{"rawValue":"2024","matchPosition":"end"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

/// 抽出元か階層番号を変えると、値の辞書の行を消す (→ docs/archive.md「軸の値の辞書と導出」)。
/// 名前や絞り込みの設定だけの変更では消さない。
#[sqlx::test]
async fn axis_update_clears_values_when_source_or_dir_level_changes(pool: SqlitePool) {
    let dir = temp_test_dir("axis-update-clears-values");

    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;

    let axis_id = create_axis(
        app.clone(),
        &cookie,
        id,
        r#"{"name":"年度","source":"dirLevel","dirLevel":1,"position":0}"#,
    )
    .await;

    let axis_url = format!("/api/v1/contents/{id}/axes/{axis_id}");
    let values = r#"{"values":[{"rawValue":"2024","displayName":"2024年度"}]}"#;
    let put_axis = |body: &'static str| send_json(app.clone(), "PUT", &axis_url, &cookie, body);
    let count_values = || async {
        sqlx::query_scalar!(
            "SELECT COUNT(*) FROM archive_axis_values WHERE axis_id = ?",
            axis_id
        )
        .fetch_one(&pool)
        .await
        .expect("辞書の行を数えられなかった")
    };

    put_axis_values(app.clone(), &cookie, id, axis_id, values).await;

    // 名前と絞り込みだけの変更では残る。
    let (status, body) = put_axis(
        r#"{"name":"年","source":"dirLevel","dirLevel":1,"filterable":false,"position":0}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(count_values().await, 1);

    // 階層番号の変更で消える。
    let (status, body) =
        put_axis(r#"{"name":"年","source":"dirLevel","dirLevel":2,"position":0}"#).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(count_values().await, 0);

    // 抽出元の変更で消える。
    put_axis_values(app.clone(), &cookie, id, axis_id, values).await;
    let (status, body) = put_axis(r#"{"name":"年","source":"filenameWord","position":0}"#).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(count_values().await, 0);
}

/// `dir_level` 軸の値の並び順は、辞書の `position` 順 → 辞書に無い生の値の昇順
/// (→ docs/archive.md「軸の値の辞書と導出」)。辞書にあってもアイテムに現れない値は返さない。
#[sqlx::test]
async fn axis_values_dir_level_orders_dictionary_by_position_then_raw_values(pool: SqlitePool) {
    let dir = test_support::project_temp_dir("api-test", "axis-values-dir-level-order");
    for year in ["2022", "2023", "2024", "2025"] {
        std::fs::create_dir_all(dir.path().join(year)).expect("登録先を作れなかった");
        std::fs::write(dir.path().join(year).join("listening.mp3"), b"")
            .expect("ファイルを作れなかった");
    }
    // 階層が足りないアイテムは「未設定」で、値の一覧には出ない。
    std::fs::write(dir.path().join("readme.mp3"), b"").expect("ファイルを作れなかった");
    register_root(&pool, dir.path()).await;

    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, dir.path(), None).await;
    let (status, body) = rescan(app.clone(), &cookie, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let axis_id = create_axis(
        app.clone(),
        &cookie,
        id,
        r#"{"name":"年度","source":"dirLevel","dirLevel":1,"position":0}"#,
    )
    .await;

    put_axis_values(app.clone(), &cookie, id, axis_id, r#"{"values":[{"rawValue":"2025"},{"rawValue":"1999"},{"rawValue":"2023","displayName":"2023年度"}]}"#).await;

    let (status, body) = send_empty(
        app,
        "GET",
        &format!("/api/v1/contents/{id}/axes/{axis_id}/values"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let values: serde_json::Value = serde_json::from_str(&body).expect("値の辞書を読めなかった");
    assert_eq!(
        values,
        serde_json::json!([
            {"rawValue": "2025", "displayName": null, "matchPosition": "anywhere"},
            {"rawValue": "2023", "displayName": "2023年度", "matchPosition": "anywhere"},
            {"rawValue": "2022", "displayName": null, "matchPosition": "anywhere"},
            {"rawValue": "2024", "displayName": null, "matchPosition": "anywhere"},
        ]),
        "{body}"
    );
}

/// `filename_word` 軸は辞書の行そのものが照合語リストなので、辞書をそのまま返す
/// (アイテムの有無に関わらない、→ docs/archive.md「軸の値の辞書と導出」)。
#[sqlx::test]
async fn axis_values_filename_word_returns_dictionary_only(pool: SqlitePool) {
    let dir = temp_test_dir("axis-values-filename-word");

    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;

    let axis_id = create_axis(
        app.clone(),
        &cookie,
        id,
        r#"{"name":"資料種別","source":"filenameWord","position":0}"#,
    )
    .await;

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}/axes/{axis_id}/values"),
        &cookie,
        r#"{"values":[{"rawValue":"listening"},{"rawValue":"script","displayName":"解答"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let values: serde_json::Value = serde_json::from_str(&body).expect("値の辞書を読めなかった");
    assert_eq!(
        values,
        serde_json::json!([
            {"rawValue": "listening", "displayName": null, "matchPosition": "anywhere"},
            {"rawValue": "script", "displayName": "解答", "matchPosition": "anywhere"},
        ]),
        "{body}"
    );

    let (status, body) = send_empty(
        app,
        "GET",
        &format!("/api/v1/contents/{id}/axes/{axis_id}/values"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let values: serde_json::Value = serde_json::from_str(&body).expect("値の辞書を読めなかった");
    assert_eq!(
        values.as_array().expect("配列でなかった").len(),
        2,
        "{body}"
    );
}

/// 一括更新の配列内で `rawValue` が重複していれば422にする。UNIQUE制約違反(500)に
/// させないため、DBに書く前にアプリ側で検出する (→ docs/archive.md「エンドポイント一覧」)。
/// 空・上限・照合する位置の場合分けは `normalize_axis_values` の単体テストで見る。
#[sqlx::test]
async fn axis_values_replace_rejects_duplicate_raw_value(pool: SqlitePool) {
    let dir = temp_test_dir("axis-values-duplicate");

    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;

    let axis_id = create_axis(
        app.clone(),
        &cookie,
        id,
        r#"{"name":"資料種別","source":"filenameWord","position":0}"#,
    )
    .await;

    let (status, body) = send_json(
        app,
        "PUT",
        &format!("/api/v1/contents/{id}/axes/{axis_id}/values"),
        &cookie,
        r#"{"values":[{"rawValue":"listening"},{"rawValue":"listening"}]}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
}

/// 値の辞書の一括更新は、アーカイブの作成者ならできる。他人のアーカイブは403
/// (→ docs/access.md「ロールと操作」)。
#[sqlx::test]
async fn axis_values_replace_is_limited_to_the_archive_creator(pool: SqlitePool) {
    insert_admin(&pool, "admin-alice", "correct-password").await;
    insert_user(&pool, "bob", "correct-password").await;
    let dir = temp_test_dir("axis-values-regular-user");
    let own_dir = temp_test_dir("axis-values-own-archive");

    let app = test_app(pool).await;
    let admin_cookie = login(app.clone(), "admin-alice", "correct-password").await;
    let id = create_archive(app.clone(), &admin_cookie, &dir, None).await;
    let axis = r#"{"name":"資料種別","source":"filenameWord","position":0}"#;
    let axis_id = create_axis(app.clone(), &admin_cookie, id, axis).await;
    let values = r#"{"values":[{"rawValue":"listening"}]}"#;

    let user_cookie = login(app.clone(), "bob", "correct-password").await;
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}/axes/{axis_id}/values"),
        &user_cookie,
        values,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let own_id = create_archive(app.clone(), &user_cookie, &own_dir, None).await;
    let own_axis_id = create_axis(app.clone(), &user_cookie, own_id, axis).await;
    put_axis_values(app, &user_cookie, own_id, own_axis_id, values).await;
}

// --- 軸を設定するときの手がかり (階層ごとの値・ファイル名の語・値の辞書のプレビュー) ---

/// 登録先に空のファイルを作る。`rel_path` は `/` 区切り。
fn write_empty_files(dir: &std::path::Path, rel_paths: &[&str]) {
    for rel_path in rel_paths {
        let path = dir.join(rel_path);
        std::fs::create_dir_all(path.parent().expect("ファイルの親が無い"))
            .expect("登録先を作れなかった");
        std::fs::write(path, b"").expect("ファイルを作れなかった");
    }
}

/// アーカイブではないコンテンツ (link) を作り、その id を返す。
async fn create_link(app: Router, cookie: &str) -> i64 {
    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/contents",
        cookie,
        r#"{"type":"link","title":"外部","url":"https://example.com"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    extract_id(&body)
}

/// フォルダーの階層ごとに、異なる値の数・その階層を持つアイテム数・値の例を返す。
/// アイテムが無ければ空配列。アーカイブでない・存在しない id は404。
#[sqlx::test]
async fn archive_dir_levels_summarizes_values_per_level(pool: SqlitePool) {
    let dir = temp_test_dir("archive-dir-levels");

    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;
    let url = format!("/api/v1/contents/{id}/dir-levels");

    let (status, body) = send_empty(app.clone(), "GET", &url, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse_json(&body), serde_json::json!([]), "{body}");

    write_empty_files(
        &dir,
        &[
            "2024/第1回/a.mp3",
            "2024/第2回/b.mp3",
            "2023/第1回/c.mp3",
            "top.mp3",
        ],
    );
    let (status, body) = rescan(app.clone(), &cookie, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send_empty(app.clone(), "GET", &url, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        parse_json(&body),
        serde_json::json!([
            {
                "level": 1,
                "valueCount": 2,
                "itemCount": 3,
                "samples": [{"value": "2024", "count": 2}, {"value": "2023", "count": 1}],
            },
            {
                "level": 2,
                "valueCount": 2,
                "itemCount": 3,
                "samples": [{"value": "第1回", "count": 2}, {"value": "第2回", "count": 1}],
            },
        ]),
        "{body}"
    );

    let link_id = create_link(app.clone(), &cookie).await;
    for uri in [
        format!("/api/v1/contents/{link_id}/dir-levels"),
        "/api/v1/contents/999999/dir-levels".to_string(),
    ] {
        let (status, body) = send_empty(app.clone(), "GET", &uri, &cookie).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}: {body}");
    }
}

/// ファイル名によく出る語を、含むアイテム数と既定で隠すかを添えて返す。
/// 大文字小文字だけが違う語はまとめる。アーカイブでない・存在しない id は404。
#[sqlx::test]
async fn archive_filename_words_counts_words_in_file_names(pool: SqlitePool) {
    let dir = temp_test_dir("archive-filename-words");

    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;
    let url = format!("/api/v1/contents/{id}/filename-words");

    let (status, body) = send_empty(app.clone(), "GET", &url, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        parse_json(&body),
        serde_json::json!({"itemCount": 0, "words": []}),
        "{body}"
    );

    write_empty_files(
        &dir,
        &[
            "2024/Listening_1.mp3",
            "2024/listening_2.mp3",
            "2023/Listening_1.mp3",
            "2023/script.pdf",
        ],
    );
    let (status, body) = rescan(app.clone(), &cookie, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send_empty(app.clone(), "GET", &url, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        parse_json(&body),
        serde_json::json!({
            "itemCount": 4,
            "words": [
                {"word": "Listening", "count": 3, "hiddenByDefault": false},
                {"word": "1", "count": 2, "hiddenByDefault": true},
                {"word": "2", "count": 1, "hiddenByDefault": true},
                {"word": "script", "count": 1, "hiddenByDefault": true},
            ],
        }),
        "{body}"
    );

    let link_id = create_link(app.clone(), &cookie).await;
    for uri in [
        format!("/api/v1/contents/{link_id}/filename-words"),
        "/api/v1/contents/999999/filename-words".to_string(),
    ] {
        let (status, body) = send_empty(app.clone(), "GET", &uri, &cookie).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}: {body}");
    }
}

/// 保存前の値の辞書で、一致した件数・未設定の件数・未設定の例と、辞書の行ごとの件数・例を返す。
/// 書き込みはしない。
#[sqlx::test]
async fn axis_values_preview_counts_matches_without_saving(pool: SqlitePool) {
    let dir = temp_test_dir("axis-values-preview");

    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;
    let word_axis_id = create_axis(
        app.clone(),
        &cookie,
        id,
        r#"{"name":"種別","source":"filenameWord","position":0}"#,
    )
    .await;
    let dir_axis_id = create_axis(
        app.clone(),
        &cookie,
        id,
        r#"{"name":"年度","source":"dirLevel","dirLevel":1,"position":1}"#,
    )
    .await;
    let preview_url = |axis_id: i64| format!("/api/v1/contents/{id}/axes/{axis_id}/values/preview");
    let values =
        r#"{"values":[{"rawValue":"listening","displayName":"リスニング","matchPosition":"end"}]}"#;

    // アイテムが無ければすべて0。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &preview_url(word_axis_id),
        &cookie,
        values,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        parse_json(&body),
        serde_json::json!({
            "itemCount": 0,
            "matched": 0,
            "unset": 0,
            "unsetExamples": [],
            "rows": [{"matched": 0, "examples": []}],
        }),
        "{body}"
    );

    write_empty_files(
        &dir,
        &[
            "2024/part1_LISTENING.mp3",
            "2024/listening_script.pdf",
            "2023/answer.pdf",
            "readme.pdf",
        ],
    );
    let (status, body) = rescan(app.clone(), &cookie, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // 送られてきた行の照合する位置 (末尾) で照合する。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &preview_url(word_axis_id),
        &cookie,
        values,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut parsed = parse_json(&body);
    // 例のアイテムは管理画面から開けるよう id を持つ。値は採番しだいなので、有ることだけ見る。
    for row in parsed["rows"].as_array_mut().expect("rowsが配列でなかった") {
        for example in row["examples"]
            .as_array_mut()
            .expect("examplesが配列でなかった")
        {
            let example = example.as_object_mut().expect("例がオブジェクトでなかった");
            assert!(example.remove("id").is_some_and(|id| id.is_i64()), "{body}");
        }
    }
    assert_eq!(
        parsed,
        serde_json::json!({
            "itemCount": 4,
            "matched": 1,
            "unset": 3,
            "unsetExamples": ["2023/answer.pdf", "2024/listening_script.pdf", "readme.pdf"],
            "rows": [{"matched": 1, "examples": [{"relPath": "2024/part1_LISTENING.mp3"}]}],
        }),
        "{body}"
    );

    // 行ごとの件数は、そのアイテムの値を決めた行 (最長一致) にだけ数える。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &preview_url(word_axis_id),
        &cookie,
        r#"{"values":[{"rawValue":"script"},{"rawValue":"listening"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let parsed = parse_json(&body);
    let row_paths = |index: usize| -> Vec<&str> {
        parsed["rows"][index]["examples"]
            .as_array()
            .expect("examplesが配列でなかった")
            .iter()
            .map(|example| example["relPath"].as_str().expect("relPathが無かった"))
            .collect()
    };
    assert_eq!(parsed["rows"][0]["matched"], 0, "{body}");
    assert_eq!(row_paths(0), Vec::<&str>::new(), "{body}");
    assert_eq!(parsed["rows"][1]["matched"], 2, "{body}");
    assert_eq!(
        row_paths(1),
        vec!["2024/listening_script.pdf", "2024/part1_LISTENING.mp3"],
        "{body}"
    );

    let (status, body) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/axes/{word_axis_id}/values"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse_json(&body), serde_json::json!([]), "{body}");

    // フォルダーの階層の軸では、階層が足りないアイテムが未設定。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &preview_url(dir_axis_id),
        &cookie,
        r#"{"values":[]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        parse_json(&body),
        serde_json::json!({
            "itemCount": 4,
            "matched": 3,
            "unset": 1,
            "unsetExamples": ["readme.pdf"],
            "rows": [],
        }),
        "{body}"
    );
}

/// プレビューの検証は一括更新と同じ `normalize_axis_values` を通る。所属しない軸・アーカイブでない id は404。
#[sqlx::test]
async fn axis_values_preview_rejects_invalid_values_and_foreign_axes(pool: SqlitePool) {
    let dir_a = temp_test_dir("axis-values-preview-a");
    let dir_b = temp_test_dir("axis-values-preview-b");
    std::fs::create_dir_all(&dir_a).expect("登録先を作れなかった");
    std::fs::create_dir_all(&dir_b).expect("登録先を作れなかった");

    let (app, cookie) = admin_app(&pool).await;
    let id_a = create_archive(app.clone(), &cookie, &dir_a, None).await;
    let id_b = create_archive(app.clone(), &cookie, &dir_b, None).await;
    let axis_id = create_axis(
        app.clone(),
        &cookie,
        id_a,
        r#"{"name":"種別","source":"filenameWord","position":0}"#,
    )
    .await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/v1/contents/{id_a}/axes/{axis_id}/values/preview"),
        &cookie,
        r#"{"values":[{"rawValue":"listening"},{"rawValue":" listening "}]}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );

    let link_id = create_link(app.clone(), &cookie).await;
    for archive_id in [id_b, link_id, 999_999] {
        let (status, body) = send_json(
            app.clone(),
            "POST",
            &format!("/api/v1/contents/{archive_id}/axes/{axis_id}/values/preview"),
            &cookie,
            r#"{"values":[]}"#,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{archive_id}: {body}");
    }
}

const IMPORT_AXES_BODY: &str = r#"{
    "axes": [
        {"name":"年度","source":"dirLevel","dirLevel":1,"values":[{"rawValue":"2024","displayName":"2024年度"}]},
        {"name":"種別","source":"filenameWord","values":[{"rawValue":"listening","displayName":"リスニング","matchPosition":"end"}]}
    ],
    "titleTemplate":"{年度} {種別}"
}"#;

#[sqlx::test]
async fn axes_import_previews_without_saving_then_replaces_axes_and_template(pool: SqlitePool) {
    let dir = temp_test_dir("axes-import");
    let (app, cookie) = admin_app(&pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;
    create_axis(
        app.clone(),
        &cookie,
        id,
        r#"{"name":"旧","source":"filenameWord","position":0}"#,
    )
    .await;
    write_empty_files(&dir, &["2024/part1_listening.mp3", "2023/answer.pdf"]);
    let (status, body) = rescan(app.clone(), &cookie, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/v1/contents/{id}/axes/import/preview"),
        &cookie,
        IMPORT_AXES_BODY,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        parse_json(&body),
        serde_json::json!({
            "itemCount": 2,
            "axes": [
                {"name": "年度", "matched": 2, "unset": 0},
                {"name": "種別", "matched": 1, "unset": 1},
            ],
            "examples": [
                {"relPath": "2023/answer.pdf", "title": "answer.pdf", "values": ["2023", null]},
                {"relPath": "2024/part1_listening.mp3", "title": "2024年度 リスニング", "values": ["2024年度", "リスニング"]},
            ],
            "replacesExisting": true,
        }),
        "{body}"
    );
    let (_, body) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/axes"),
        &cookie,
    )
    .await;
    assert!(
        body.contains(r#""name":"旧""#),
        "予告では保存しない: {body}"
    );

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/contents/{id}/axes"),
        &cookie,
        IMPORT_AXES_BODY,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, body) = send_empty(
        app.clone(),
        "GET",
        &format!("/api/v1/contents/{id}/axes"),
        &cookie,
    )
    .await;
    let names: Vec<_> = parse_json(&body)
        .as_array()
        .expect("axes array")
        .iter()
        .map(|axis| axis["name"].as_str().expect("axis name").to_string())
        .collect();
    assert_eq!(names, ["年度", "種別"]);
    let (status, body) =
        send_empty(app, "GET", &format!("/api/v1/contents/{id}/items"), &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""title":"2024年度 リスニング""#), "{body}");
}

#[sqlx::test]
async fn axes_import_rejects_invalid_definitions_and_other_users(pool: SqlitePool) {
    insert_user(&pool, "bob", "correct-password").await;
    let dir = temp_test_dir("axes-import-invalid");
    let app = test_app(pool.clone()).await;
    let cookie = admin_cookie(app.clone(), &pool).await;
    let id = create_archive(app.clone(), &cookie, &dir, None).await;
    let preview_url = format!("/api/v1/contents/{id}/axes/import/preview");
    let replace_url = format!("/api/v1/contents/{id}/axes");

    // 決まりの場合分けは `validate_import` の単体テストで見る。ここは両方の入口がつながっていることだけ。
    let missing_level = r#"{"axes":[{"name":"年度","source":"dirLevel"}]}"#;
    for (method, url) in [("POST", &preview_url), ("PUT", &replace_url)] {
        let (status, body) = send_json(app.clone(), method, url, &cookie, missing_level).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{method} {url}: {body}"
        );
        assert!(
            body.contains(r#""kind":"importAxisInvalid","name":"年度""#),
            "{method} {url}: {body}"
        );
    }

    // 他人のアーカイブは、予告も置き換えもできない。
    let user_cookie = login(app.clone(), "bob", "correct-password").await;
    for (method, url) in [("POST", &preview_url), ("PUT", &replace_url)] {
        let (status, body) =
            send_json(app.clone(), method, url, &user_cookie, IMPORT_AXES_BODY).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {url}: {body}");
    }
}
