//! URL のファイルの中継 (→ docs/ui.md「URL のファイル」)。中継する URL の決まりは `src/api/remote_file.rs` の
//! 単体テストで見る。ここは外へつながないよう、公開アドレスでない URL だけを使う。

use super::*;

async fn insert_link_to(pool: &SqlitePool, url: &str, visibility: &str) -> i64 {
    let id = insert_link_by(pool, "資料", visibility, None, None).await;
    sqlx::query("UPDATE contents SET url = ? WHERE id = ?")
        .bind(url)
        .bind(id)
        .execute(pool)
        .await
        .expect("リンクの URL を書き換えられなかった");
    id
}

/// LAN の相手には中継でもつながない (`link_title::PublicOnlyResolver`)。
#[sqlx::test]
async fn remote_file_on_a_private_address_is_not_relayed(pool: SqlitePool) {
    let id = insert_link_to(&pool, "http://127.0.0.1:9/handout.pdf", "public").await;
    let app = test_app(pool).await;

    let (status, body) = send_anon(app, "GET", &format!("/api/v1/contents/{id}/remote")).await;
    assert_error(status, &body, StatusCode::NOT_FOUND, "not_found");
}

/// 閲覧の権限は、取りに行く前に確かめる。
#[sqlx::test]
async fn remote_file_of_a_link_for_signed_in_users_requires_login(pool: SqlitePool) {
    let id = insert_link_to(&pool, "http://127.0.0.1:9/handout.pdf", "authenticated").await;
    let app = test_app(pool).await;

    let (status, body) = send_anon(app, "GET", &format!("/api/v1/contents/{id}/remote")).await;
    assert_error(status, &body, StatusCode::UNAUTHORIZED, "unauthorized");
}

/// ページのタイトルが取れないファイルの URL は、ホスト名でなくファイル名をタイトルにする。
#[sqlx::test]
async fn link_to_a_file_is_titled_with_the_file_name(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app,
        "POST",
        "/api/v1/contents",
        &cookie,
        r#"{"type":"link","url":"http://192.168.0.10/handouts/%E8%B3%87%E6%96%99.pdf"}"#,
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(parse_json(&body)["title"], "資料.pdf", "{body}");
}
