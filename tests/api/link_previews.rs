//! リンクのカードの情報 (→ docs/ui.md「リンクのカード」)。取りに行く決まりは `src/api/link_preview.rs` の単体テストで見る。
//! ここは外へつながないよう、期限の切れていない情報を先に入れておく。

use super::*;

const IMAGE_FILE: &str = "0000000000000000000000000000000000000000000000000000000000000000.jpg";

#[sqlx::test]
async fn home_list_carries_the_remembered_link_preview(pool: SqlitePool) {
    insert_link_by(&pool, "リンク", "public", None, None).await;
    insert_fresh_preview(&pool, "https://example.com", Some(IMAGE_FILE)).await;
    let app = test_app(pool).await;

    let (status, body) = send_anon(app, "GET", "/api/v1/contents").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let json = parse_json(&body);
    let preview = &json[0]["preview"];
    assert_eq!(preview["siteName"], "Example Site", "{json}");
    assert_eq!(
        preview["imageUrl"],
        format!("/api/v1/link-previews/files/{IMAGE_FILE}"),
        "{json}"
    );
}

/// 見られるリンクの今の情報を返す。見られない id を飛ばす場合分けは `contents::viewable_link_url` の単体テストで見る。
#[sqlx::test]
async fn refresh_answers_with_the_preview_of_a_visible_link(pool: SqlitePool) {
    let id = insert_link_by(&pool, "公開", "public", None, None).await;
    insert_fresh_preview(&pool, "https://example.com", Some(IMAGE_FILE)).await;
    let app = test_app(pool).await;

    let (status, body) = send_anon_json(
        app,
        "POST",
        "/api/v1/link-previews/refresh",
        &format!(r#"{{"contentIds":[{id}]}}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let json = parse_json(&body);
    assert_eq!(json[0]["url"], "https://example.com", "{json}");
    assert_eq!(json[0]["preview"]["title"], "Example", "{json}");
}

#[sqlx::test]
async fn refresh_rejects_too_many_links(pool: SqlitePool) {
    let app = test_app(pool).await;
    let ids = (1..=201)
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let (status, body) = send_anon_json(
        app,
        "POST",
        "/api/v1/link-previews/refresh",
        &format!(r#"{{"contentIds":[{ids}]}}"#),
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
}

#[sqlx::test]
async fn preview_files_are_served_only_by_hash_name_and_only_when_remembered(pool: SqlitePool) {
    let app = test_app(pool).await;

    let (status, body) =
        send_anon(app.clone(), "GET", "/api/v1/link-previews/files/weblav.db").await;
    assert_error(status, &body, StatusCode::NOT_FOUND, "not_found");
    let (status, body) = send_anon(
        app,
        "GET",
        &format!("/api/v1/link-previews/files/{IMAGE_FILE}"),
    )
    .await;
    assert_error(status, &body, StatusCode::NOT_FOUND, "not_found");
}
