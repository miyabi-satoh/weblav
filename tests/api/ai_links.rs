//! AI が集めるリンク: グループの設定・集め直し・見せ方 (→ docs/ai.md「AI が集めるリンク」)。
//! 窓口は `ai.rs` の偽のサーバーに差し替える。関心の言葉や答えの読み方の場合分けは `src/api/ai_links.rs` の単体テスト。

use super::ai::{AiApp, ai_app, get_json, usage_rows};
use super::*;

/// 検索の候補 3 件。2件目は、グループに登録してあるリンク (`insert_link_by` の URL) と同じ。
const SEARCH_BODY: &str = r#"{"results":[
    {"title":"A","url":"https://a.example/","date":"2026-01-02","snippet":"about a"},
    {"title":"Registered","url":"https://example.com","snippet":""},
    {"title":"C","url":"https://c.example/","snippet":"about c"}
]}"#;

/// AI の答え。`text` を文として返す、Gemini の Interactions API の形。
fn answer(text: &str) -> String {
    serde_json::json!({
        "steps": [{ "type": "model_output", "content": [{ "type": "text", "text": text }] }]
    })
    .to_string()
}

fn settings_uri(id: i64) -> String {
    format!("/api/v1/contents/{id}/ai-links/settings")
}

fn refresh_uri(id: i64) -> String {
    format!("/api/v1/contents/{id}/ai-links/refresh")
}

fn links_uri(id: i64) -> String {
    format!("/api/v1/contents/{id}/ai-links")
}

/// 公開のグループを作り、関心の言葉を付ける。
async fn group_with_interests(app: &Router, pool: &SqlitePool, cookie: &str) -> i64 {
    let id = insert_group_under(pool, "集める", "public", None).await;
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &settings_uri(id),
        cookie,
        r#"{"interests":"Rust 非同期\n家庭菜園"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    id
}

async fn refresh(app: &Router, id: i64, cookie: &str) -> (StatusCode, String) {
    send_json(
        app.clone(),
        "POST",
        &refresh_uri(id),
        cookie,
        r#"{"language":"ja"}"#,
    )
    .await
}

/// 設定 → 集め直す → 匿名で見る、の一巡。検索は2回、AI は1回呼ばれ、選んだ順に並ぶ。
#[sqlx::test]
async fn refreshing_collects_the_links_the_ai_picked(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with_interests(&app, &pool, &cookie).await;
    insert_link_by(&pool, "登録済み", "public", None, Some(id)).await;
    {
        let mut fake = fake.lock().expect("鍵が毒された");
        fake.search = (StatusCode::OK, SEARCH_BODY.to_string(), Some(10));
        // 候補は、登録済みの URL と2回目の検索の重なりを外した A・C の順。
        fake.ai = (StatusCode::OK, answer("[2, 1]"), Some(20));
    }

    let before = get_json(app.clone(), &links_uri(id), &cookie).await;
    assert_eq!(before["enabled"], true, "{before}");
    assert_eq!(before["refreshedAt"], serde_json::Value::Null, "{before}");
    assert_eq!(before["canRefresh"], true, "{before}");

    let (status, body) = refresh(&app, id, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let refreshed: serde_json::Value = serde_json::from_str(&body).expect("JSON でない");
    let urls: Vec<&str> = refreshed["links"]
        .as_array()
        .expect("配列でない")
        .iter()
        .map(|link| link["url"].as_str().expect("URL が無い"))
        .collect();
    assert_eq!(urls, ["https://c.example/", "https://a.example/"], "{body}");
    assert_eq!(refreshed["links"][0]["title"], "C", "{body}");
    assert!(refreshed["refreshedAt"].is_i64(), "{body}");

    {
        let fake = fake.lock().expect("鍵が毒された");
        assert_eq!(fake.searches.len(), 2, "{:?}", fake.searches);
        assert_eq!(
            fake.searches[0],
            serde_json::json!({
                "queries": ["Rust 非同期", "家庭菜園"], "country": "JP", "language": "ja"
            })
        );
        assert_eq!(fake.searches[1]["recency"], "year");
        assert_eq!(fake.prompts.len(), 1, "{:?}", fake.prompts);
        let prompt = fake.prompts[0]["user"].as_str().expect("文が無い");
        assert!(prompt.contains("1. A\n"), "{prompt}");
        assert!(prompt.contains("2. C\n"), "{prompt}");
        assert!(!prompt.contains("Registered"), "{prompt}");
    }
    let kinds: Vec<(String, i64)> = usage_rows(&pool)
        .await
        .into_iter()
        .map(|(_, _, kind, used)| (kind, used))
        .collect();
    assert_eq!(
        kinds,
        [
            ("links".to_string(), 10),
            ("links".to_string(), 10),
            ("links".to_string(), 20)
        ]
    );

    // 匿名でも、公開のグループの集めたリンクは見られる。集め直しはできない。
    let (status, body) = send_anon(app.clone(), "GET", &links_uri(id)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let seen: serde_json::Value = serde_json::from_str(&body).expect("JSON でない");
    assert_eq!(seen["links"].as_array().map(Vec::len), Some(2), "{body}");
    assert_eq!(seen["canRefresh"], false, "{body}");

    // 管理の一覧の行に、集めるグループの印が付く。
    let listed = get_json(app.clone(), "/api/v1/admin/contents", &cookie).await;
    let group = listed
        .as_array()
        .expect("配列でない")
        .iter()
        .find(|content| content["id"] == id)
        .expect("グループが一覧に無い");
    assert_eq!(group["aiLinks"], true, "{group}");
}

/// 集めたリンクをグループに登録すると、AI の分からは消える (二重に並べない)。
#[sqlx::test]
async fn a_collected_link_registered_in_the_group_is_not_shown_twice(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with_interests(&app, &pool, &cookie).await;
    {
        let mut fake = fake.lock().expect("鍵が毒された");
        fake.search = (StatusCode::OK, SEARCH_BODY.to_string(), Some(10));
        fake.ai = (StatusCode::OK, answer("[2]"), Some(20));
    }
    let (status, body) = refresh(&app, id, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("https://example.com"), "{body}");

    insert_link_by(&pool, "登録した", "public", None, Some(id)).await;
    let seen = get_json(app.clone(), &links_uri(id), &cookie).await;
    assert_eq!(seen["links"].as_array().map(Vec::len), Some(0), "{seen}");
}

/// 設定を外すと、集めたリンクも消え、集め直しは 404 になる。
#[sqlx::test]
async fn turning_the_setting_off_removes_the_collected_links(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with_interests(&app, &pool, &cookie).await;
    {
        let mut fake = fake.lock().expect("鍵が毒された");
        fake.search = (StatusCode::OK, SEARCH_BODY.to_string(), Some(10));
        fake.ai = (StatusCode::OK, answer("[1]"), Some(20));
    }
    let (status, body) = refresh(&app, id, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &settings_uri(id),
        &cookie,
        r#"{"interests":null}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let settings = get_json(app.clone(), &settings_uri(id), &cookie).await;
    assert_eq!(settings["interests"], serde_json::Value::Null, "{settings}");
    let seen = get_json(app.clone(), &links_uri(id), &cookie).await;
    assert_eq!(seen["enabled"], false, "{seen}");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ai_links")
        .fetch_one(&pool)
        .await
        .expect("数えられなかった");
    assert_eq!(count, 0);

    let (status, body) = refresh(&app, id, &cookie).await;
    assert_error(status, &body, StatusCode::NOT_FOUND, "not_found");
}

#[sqlx::test]
async fn settings_reject_empty_interests_and_non_groups(pool: SqlitePool) {
    let AiApp { app, cookie, .. } = ai_app(&pool, true).await;
    let id = insert_group_under(&pool, "集める", "public", None).await;
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &settings_uri(id),
        &cookie,
        r#"{"interests":" \n "}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );

    let link = insert_link_by(&pool, "リンク", "public", None, None).await;
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &settings_uri(link),
        &cookie,
        r#"{"interests":"Rust"}"#,
    )
    .await;
    assert_error(status, &body, StatusCode::NOT_FOUND, "not_found");
    let (status, body) = send_anon(app.clone(), "GET", &links_uri(link)).await;
    assert_error(status, &body, StatusCode::NOT_FOUND, "not_found");
}

/// 残りを使うので、AI を使えない編集者は集め直せない。ほかの人が作ったグループの設定も変えられない。
#[sqlx::test]
async fn an_editor_without_ai_cannot_refresh_or_change_the_setting(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with_interests(&app, &pool, &cookie).await;
    insert_user(&pool, "taro", "password123").await;
    let taro = login(app.clone(), "taro", "password123").await;

    let seen = get_json(app.clone(), &links_uri(id), &taro).await;
    assert_eq!(seen["canRefresh"], false, "{seen}");
    let (status, body) = refresh(&app, id, &taro).await;
    assert_error(status, &body, StatusCode::FORBIDDEN, "ai_not_allowed");
    assert!(fake.lock().expect("鍵が毒された").searches.is_empty());

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &settings_uri(id),
        &taro,
        r#"{"interests":"Rust"}"#,
    )
    .await;
    assert_error(status, &body, StatusCode::FORBIDDEN, "forbidden");
}

/// AI の答えに番号の並びが無ければ、前に集めたリンクを残して 502 にする。
#[sqlx::test]
async fn an_answer_without_numbers_keeps_the_previous_links(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with_interests(&app, &pool, &cookie).await;
    {
        let mut fake = fake.lock().expect("鍵が毒された");
        fake.search = (StatusCode::OK, SEARCH_BODY.to_string(), Some(10));
        fake.ai = (StatusCode::OK, answer("[1]"), Some(20));
    }
    let (status, body) = refresh(&app, id, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    fake.lock().expect("鍵が毒された").ai = (StatusCode::OK, answer("選べませんでした"), Some(20));
    let (status, body) = refresh(&app, id, &cookie).await;
    assert_error(status, &body, StatusCode::BAD_GATEWAY, "ai_upstream");
    let seen = get_json(app.clone(), &links_uri(id), &cookie).await;
    assert_eq!(seen["links"].as_array().map(Vec::len), Some(1), "{seen}");
}

/// カードの取り直しは、見られるグループの集めたリンクだけを対象にする。
#[sqlx::test]
async fn link_previews_are_refreshed_for_a_viewable_group_only(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with_interests(&app, &pool, &cookie).await;
    {
        let mut fake = fake.lock().expect("鍵が毒された");
        fake.search = (StatusCode::OK, SEARCH_BODY.to_string(), Some(10));
        fake.ai = (StatusCode::OK, answer("[1]"), Some(20));
    }
    let (status, body) = refresh(&app, id, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    insert_fresh_preview(&pool, "https://a.example/", None).await;

    let request = format!(r#"{{"contentIds":[],"aiLinksGroup":{id}}}"#);
    let (status, body) = send_anon_json(
        app.clone(),
        "POST",
        "/api/v1/link-previews/refresh",
        &request,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("https://a.example/"), "{body}");

    sqlx::query("UPDATE contents SET visibility = 'authenticated' WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .expect("公開範囲を変えられなかった");
    let (status, body) = send_anon_json(
        app.clone(),
        "POST",
        "/api/v1/link-previews/refresh",
        &request,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, "[]");
}
