//! 表示しないもの (→ `api::hidden`)。判定の場合分けは単体テストで確かめ、ここでは設定の口と、判定が共有の中の
//! 操作と索引につながっていることを見る。

use super::*;

const HIDDEN_URI: &str = "/api/v1/admin/hidden-settings";

fn exclude_uri(id: i64) -> String {
    format!("{ROOTS_URI}/{id}/exclude")
}

async fn set_exclude(share: &ShareFixture, patterns: &str) {
    let (status, body) = send(
        share.app.clone(),
        roots_request(
            "PUT",
            &exclude_uri(share.id),
            &share.admin,
            Some(&format!(
                r#"{{"excludePatterns":{}}}"#,
                json_string(patterns)
            )),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse_json(&body)["excludePatterns"], patterns);
}

fn detail_kind(body: &str) -> serde_json::Value {
    parse_json(body)["error"]["detail"]["kind"].clone()
}

/// 既定のままなら既定のパターンを返し、空にするとドット始まりも見えるようになる。`null` で既定に戻る。
#[sqlx::test]
async fn site_patterns_start_from_the_default_and_can_be_emptied(pool: SqlitePool) {
    let share = share_fixture(&pool, "hidden-site").await;
    std::fs::write(share.dir.join(".env"), b"x").expect("書けなかった");
    std::fs::write(share.dir.join("memo.txt"), b"memo").expect("書けなかった");

    let (status, body) = send_empty(share.app.clone(), "GET", HIDDEN_URI, &share.admin).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let body = parse_json(&body);
    assert_eq!(body["isDefault"], true);
    assert_eq!(body["patterns"], body["defaultPatterns"]);

    let browse = |cookie: String| {
        let app = share.app.clone();
        let uri = browse_uri(share.id, "");
        async move {
            let (status, body) = send_empty(app, "GET", &uri, &cookie).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            entry_names(&body)
        }
    };
    assert_eq!(browse(share.taro.clone()).await, vec!["memo.txt"]);

    let put = |json: &'static str| {
        send(
            share.app.clone(),
            roots_request("PUT", HIDDEN_URI, &share.admin, Some(json)),
        )
    };
    let (status, body) = put(r#"{"patterns":""}"#).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse_json(&body)["isDefault"], false);
    assert_eq!(browse(share.taro.clone()).await, vec![".env", "memo.txt"]);

    let (status, body) = put(r#"{"patterns":null}"#).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse_json(&body)["isDefault"], true);

    let (status, body) = put(r#"{"patterns":"docs/private"}"#).await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
    assert_eq!(detail_kind(&body), "hiddenPatternNotName");
}

/// 共有の除外に当たるものは一覧に出ず、開けず、その名前では作れず、それを含むフォルダーは消せない。
#[sqlx::test]
async fn a_share_exclude_hides_entries_and_protects_what_holds_them(pool: SqlitePool) {
    let share = share_fixture(&pool, "hidden-share").await;
    std::fs::create_dir_all(share.dir.join("album").join("private")).expect("作れなかった");
    std::fs::write(share.dir.join("album").join("a.jpg"), b"jpg").expect("書けなかった");
    set_exclude(&share, "private/").await;
    set_editor_access(&share, "all").await;

    let (status, body) = send_empty(
        share.app.clone(),
        "GET",
        &browse_uri(share.id, "album"),
        &share.taro,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(entry_names(&body), vec!["a.jpg"]);
    let uri = browse_uri(share.id, "album/private");
    let (status, body) = send_empty(share.app.clone(), "GET", &uri, &share.admin).await;
    assert_error(status, &body, StatusCode::NOT_FOUND, "not_found");

    let (status, body) = send_json(
        share.app.clone(),
        "POST",
        &json_uri(share.id, "folders"),
        &share.taro,
        r#"{"path":"","name":"private"}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
    assert_eq!(detail_kind(&body), "entryNameHidden");

    let (status, body) = send_json(
        share.app.clone(),
        "POST",
        &json_uri(share.id, "delete/check"),
        &share.taro,
        r#"{"paths":["album"]}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
    assert_eq!(detail_kind(&body), "entryContainsHidden");
}

/// 除外を足すと、一覧の表示の索引から当たる行がすぐに外れ、フォルダーは読み直しの対象になる。
#[sqlx::test]
async fn a_new_exclude_drops_matching_rows_from_the_index(pool: SqlitePool) {
    let share = share_fixture(&pool, "hidden-index").await;
    let folder = insert_folder_by(&pool, "写真", &*share.dir, "public", None, None).await;
    sqlx::query("UPDATE contents SET list_view = 1 WHERE id = ?")
        .bind(folder)
        .execute(&pool)
        .await
        .expect("一覧の表示にできなかった");
    for rel in ["album/a.jpg", "album/private/b.jpg"] {
        sqlx::query("INSERT INTO archive_items (archive_id, rel_path) VALUES (?, ?)")
            .bind(folder)
            .bind(rel)
            .execute(&pool)
            .await
            .expect("索引を入れられなかった");
    }
    sqlx::query(
        "INSERT INTO archive_dirs (archive_id, rel_path, modified_ns) VALUES (?, 'album', 1), (?, 'album/private', 1)",
    )
    .bind(folder)
    .bind(folder)
    .execute(&pool)
    .await
    .expect("フォルダーの行を入れられなかった");

    set_exclude(&share, "private/").await;

    let items: Vec<String> =
        sqlx::query_scalar("SELECT rel_path FROM archive_items WHERE archive_id = ?")
            .bind(folder)
            .fetch_all(&pool)
            .await
            .expect("索引を読めなかった");
    assert_eq!(items, vec!["album/a.jpg"]);
    let dirs: Vec<(String, Option<i64>)> =
        sqlx::query_as("SELECT rel_path, modified_ns FROM archive_dirs WHERE archive_id = ?")
            .bind(folder)
            .fetch_all(&pool)
            .await
            .expect("フォルダーの行を読めなかった");
    assert_eq!(dirs, vec![("album".to_string(), None)]);
}
