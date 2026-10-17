//! AI が集めるリンク: グループの設定・集め直し・自動の更新・表示しない (→ docs/ai.md「AI が集めるリンク」)。
//! 窓口は `ai.rs` の偽のサーバーに差し替える。検索キーワード・自動の更新・候補の外し方の場合分けは `src/api/ai_links.rs` の単体テスト。

use super::ai::{AiApp, ai_app, get_json, usage_rows};
use super::*;

/// 検索の候補 3 件。2件目は、グループに登録してあるリンク (`insert_link_by` の URL) と同じ。
const SEARCH_BODY: &str = r#"{"results":[
    {"title":"C","url":"https://c.example/","snippet":"about c"},
    {"title":"Registered","url":"https://example.com","snippet":""},
    {"title":"A","url":"https://a.example/","date":"2026-01-02","snippet":"about a"}
]}"#;

/// `SEARCH_BODY` の A に加えて、新しいページ D が出た検索の結果。C は出なくなった。
const SEARCH_BODY_WITH_D: &str = r#"{"results":[
    {"title":"A","url":"https://a.example/","date":"2026-01-02","snippet":"about a"},
    {"title":"D","url":"https://d.example/","snippet":"about d"}
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

fn auto_refresh_uri(id: i64) -> String {
    format!("/api/v1/contents/{id}/ai-links/auto-refresh")
}

fn links_uri(id: i64) -> String {
    format!("/api/v1/contents/{id}/ai-links")
}

fn exclusions_uri(id: i64) -> String {
    format!("/api/v1/contents/{id}/ai-links/exclusions")
}

/// AI が作るクエリの答え。検索キーワードを保存した後、最初に集めるときに1回だけ呼ばれる。
const QUERIES_ANSWER: &str = r#"["Rust 非同期", "家庭菜園"]"#;

/// 検索の答えと、クエリを作る AI の答えを用意する。
fn prepare(fake: &super::ai::SharedFake, search: &str) {
    let mut fake = fake.lock().expect("鍵が毒された");
    fake.search = (StatusCode::OK, search.to_string(), Some(10));
    fake.ai = (StatusCode::OK, answer(QUERIES_ANSWER), Some(5));
}

fn set_search(fake: &super::ai::SharedFake, search: &str) {
    fake.lock().expect("鍵が毒された").search = (StatusCode::OK, search.to_string(), Some(10));
}

/// 公開のグループを作り、`settings` (JSON) で集める設定を付ける。
async fn group_with(app: &Router, pool: &SqlitePool, cookie: &str, settings: &str) -> i64 {
    let id = insert_group_under(pool, "集める", "public", None).await;
    let (status, body) = send_json(app.clone(), "PUT", &settings_uri(id), cookie, settings).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    id
}

async fn group_with_interests(app: &Router, pool: &SqlitePool, cookie: &str) -> i64 {
    group_with(
        app,
        pool,
        cookie,
        r#"{"interests":"Rust の非同期と家庭菜園"}"#,
    )
    .await
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

async fn refreshed(app: &Router, id: i64, cookie: &str) -> serde_json::Value {
    let (status, body) = refresh(app, id, cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    serde_json::from_str(&body).expect("JSON でない")
}

/// 並んだリンクの URL と、新しく足したかの印。
fn shown(links: &serde_json::Value) -> Vec<(String, bool)> {
    links["links"]
        .as_array()
        .expect("配列でない")
        .iter()
        .map(|link| {
            (
                link["url"].as_str().expect("URL が無い").to_string(),
                link["isNew"].as_bool().expect("印が無い"),
            )
        })
        .collect()
}

fn urls(links: &serde_json::Value) -> Vec<String> {
    shown(links).into_iter().map(|(url, _)| url).collect()
}

/// 設定 → 集め直す → 匿名で見る、の一巡。AI がクエリを作り、検索は2回。登録済みの URL は並べない。
/// 2回目は作ったクエリを使い回し、新しく出たページを上に足して、出なくなったページを外す。
#[sqlx::test]
async fn refreshing_adds_new_search_results_on_top_and_reuses_the_queries(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with_interests(&app, &pool, &cookie).await;
    insert_link_by(&pool, "登録済み", "public", None, Some(id)).await;
    prepare(&fake, SEARCH_BODY);

    let before = get_json(app.clone(), &links_uri(id), &cookie).await;
    assert_eq!(before["enabled"], true, "{before}");
    assert_eq!(before["refreshedAt"], serde_json::Value::Null, "{before}");
    assert_eq!(before["canRefresh"], true, "{before}");
    assert_eq!(before["canEdit"], true, "{before}");

    let first = refreshed(&app, id, &cookie).await;
    // 同じときに見つかったものは、日付のある A が先。初めて集めたときは、新しい印を付けない。
    assert_eq!(
        shown(&first),
        [
            ("https://a.example/".to_string(), false),
            ("https://c.example/".to_string(), false)
        ],
        "{first}"
    );
    assert_eq!(first["added"], 2, "{first}");
    assert_eq!(first["links"][1]["title"], "C", "{first}");
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
        let asked = fake.prompts[0]["user"].as_str().expect("文が無い");
        assert!(asked.contains("Rust の非同期と家庭菜園"), "{asked}");
        assert!(asked.contains("Language of the queries: ja"), "{asked}");
    }

    // 見つけた時刻は秒の単位なので、1回目と分かれるように待つ。
    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
    set_search(&fake, SEARCH_BODY_WITH_D);
    let second = refreshed(&app, id, &cookie).await;
    assert_eq!(
        shown(&second),
        [
            ("https://d.example/".to_string(), true),
            ("https://a.example/".to_string(), false)
        ],
        "{second}"
    );
    assert_eq!(second["added"], 1, "{second}");
    assert_eq!(fake.lock().expect("鍵が毒された").prompts.len(), 1);

    let kinds: Vec<(String, i64)> = usage_rows(&pool)
        .await
        .into_iter()
        .map(|(_, _, kind, used)| (kind, used))
        .collect();
    assert_eq!(
        kinds,
        [5, 10, 10, 10, 10].map(|used| ("links".to_string(), used))
    );

    // 匿名でも、公開のグループの集めたリンクは見られる。集め直しも、表示しないもできない。
    let (status, body) = send_anon(app.clone(), "GET", &links_uri(id)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let seen: serde_json::Value = serde_json::from_str(&body).expect("JSON でない");
    assert_eq!(seen["links"].as_array().map(Vec::len), Some(2), "{body}");
    assert_eq!(seen["canRefresh"], false, "{body}");
    assert_eq!(seen["canEdit"], false, "{body}");

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

/// 新しいページが1件も無ければ、何も入れ替えず、足した数 0 を返す (画面は「今日はここまで」と伝える)。
#[sqlx::test]
async fn a_refresh_without_new_pages_keeps_the_links(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with_interests(&app, &pool, &cookie).await;
    prepare(&fake, SEARCH_BODY);
    refreshed(&app, id, &cookie).await;

    // A だけが出た検索でも、C は外さない。
    set_search(
        &fake,
        r#"{"results":[{"title":"A","url":"https://a.example/","snippet":""}]}"#,
    );
    let again = refreshed(&app, id, &cookie).await;
    assert_eq!(again["added"], 0, "{again}");
    assert_eq!(
        urls(&again),
        [
            "https://a.example/",
            "https://c.example/",
            "https://example.com"
        ],
        "{again}"
    );
}

/// 集めたリンクをグループに登録すると、AI の分からは消える (二重に並べない)。
#[sqlx::test]
async fn a_collected_link_registered_in_the_group_is_not_shown_twice(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with_interests(&app, &pool, &cookie).await;
    prepare(&fake, SEARCH_BODY);
    let collected = refreshed(&app, id, &cookie).await;
    assert!(
        urls(&collected).contains(&"https://example.com".to_string()),
        "{collected}"
    );

    insert_link_by(&pool, "登録した", "public", None, Some(id)).await;
    let seen = get_json(app.clone(), &links_uri(id), &cookie).await;
    assert_eq!(
        urls(&seen),
        ["https://a.example/", "https://c.example/"],
        "{seen}"
    );
}

/// 1日の回数を使い切ると、集め直しは `ai_links_daily_limit` になり、検索も呼ばない。
#[sqlx::test]
async fn refreshing_stops_at_the_daily_limit(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with(
        &app,
        &pool,
        &cookie,
        r#"{"interests":"Rust","dailyLimit":1}"#,
    )
    .await;
    prepare(&fake, SEARCH_BODY);
    let first = refreshed(&app, id, &cookie).await;
    assert_eq!(first["refreshesLeft"], 0, "{first}");

    let (status, body) = refresh(&app, id, &cookie).await;
    assert_error(
        status,
        &body,
        StatusCode::TOO_MANY_REQUESTS,
        "ai_links_daily_limit",
    );
    assert_eq!(fake.lock().expect("鍵が毒された").searches.len(), 2);
}

/// 自動の更新は、時間が来ていれば匿名で開いても集め直し、使った量は自動の更新の分として残す。
/// 時間が来ていなければ何もしない。
#[sqlx::test]
async fn the_auto_refresh_runs_for_anyone_only_when_it_is_due(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with(
        &app,
        &pool,
        &cookie,
        r#"{"interests":"Rust","autoRefresh":{"mode":"interval","hours":6}}"#,
    )
    .await;
    prepare(&fake, SEARCH_BODY);

    let (status, body) = send_anon(app.clone(), "GET", &links_uri(id)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""autoRefreshDue":true"#), "{body}");

    let (status, body) = send_anon_json(app.clone(), "POST", &auto_refresh_uri(id), "{}").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let collected: serde_json::Value = serde_json::from_str(&body).expect("JSON でない");
    assert_eq!(collected["added"], 3, "{collected}");
    assert_eq!(collected["refreshedByAuto"], true, "{collected}");
    assert_eq!(collected["autoRefreshDue"], false, "{collected}");
    let rows = usage_rows(&pool).await;
    assert!(
        rows.iter()
            .all(|(user_id, username, _, _)| user_id.is_none() && username.is_empty()),
        "{rows:?}"
    );

    let (status, body) = send_anon_json(app.clone(), "POST", &auto_refresh_uri(id), "{}").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""added":null"#), "{body}");
    assert_eq!(fake.lock().expect("鍵が毒された").searches.len(), 2);
}

/// 表示しないにしたページとサイトは並ばず、集め直しても出ない。取り消すと、また並ぶ。
#[sqlx::test]
async fn excluded_pages_and_sites_are_hidden_until_the_exclusion_is_removed(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with_interests(&app, &pool, &cookie).await;
    prepare(&fake, SEARCH_BODY);
    refreshed(&app, id, &cookie).await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        &exclusions_uri(id),
        &cookie,
        r#"{"url":"https://a.example/","kind":"page"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let page: serde_json::Value = serde_json::from_str(&body).expect("JSON でない");
    assert_eq!(page["title"], "A", "{page}");
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &exclusions_uri(id),
        &cookie,
        r#"{"url":"https://c.example/","kind":"site"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""value":"c.example""#), "{body}");

    let seen = get_json(app.clone(), &links_uri(id), &cookie).await;
    assert_eq!(urls(&seen), ["https://example.com"], "{seen}");
    let settings = get_json(app.clone(), &settings_uri(id), &cookie).await;
    assert_eq!(
        settings["exclusions"].as_array().map(Vec::len),
        Some(2),
        "{settings}"
    );

    // 集め直しても、表示しないものは足さない。
    set_search(
        &fake,
        r#"{"results":[
            {"title":"C2","url":"https://www.c.example/2","snippet":""},
            {"title":"E","url":"https://e.example/","snippet":""}
        ]}"#,
    );
    let again = refreshed(&app, id, &cookie).await;
    assert_eq!(again["added"], 1, "{again}");
    assert_eq!(urls(&again), ["https://e.example/"], "{again}");

    let (status, body) = send(
        app.clone(),
        build_request(
            "DELETE",
            &format!("{}/{}", exclusions_uri(id), page["id"]),
            Some(&cookie),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, body) = send(
        app.clone(),
        build_request(
            "DELETE",
            &format!("{}/{}", exclusions_uri(id), page["id"]),
            Some(&cookie),
            None,
        ),
    )
    .await;
    assert_error(status, &body, StatusCode::NOT_FOUND, "not_found");
}

/// 設定を外すと、集めたリンクも消え、集め直しは 404 になる。
#[sqlx::test]
async fn turning_the_setting_off_removes_the_collected_links(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with_interests(&app, &pool, &cookie).await;
    prepare(&fake, SEARCH_BODY);
    refreshed(&app, id, &cookie).await;

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

/// 検索キーワードを変えると、作ったクエリを捨て、次に集めるときに作り直す。
#[sqlx::test]
async fn changing_the_interests_makes_the_queries_again(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with_interests(&app, &pool, &cookie).await;
    prepare(&fake, SEARCH_BODY);
    refreshed(&app, id, &cookie).await;
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &settings_uri(id),
        &cookie,
        r#"{"interests":"トマトの育て方","dailyLimit":3}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    refreshed(&app, id, &cookie).await;
    let prompts = fake.lock().expect("鍵が毒された").prompts.clone();
    assert_eq!(prompts.len(), 2, "{prompts:?}");
    let asked = prompts[1]["user"].as_str().expect("文が無い");
    assert!(asked.contains("トマトの育て方"), "{asked}");
}

#[sqlx::test]
async fn settings_reject_bad_values_and_non_groups(pool: SqlitePool) {
    let AiApp { app, cookie, .. } = ai_app(&pool, true).await;
    let id = insert_group_under(&pool, "集める", "public", None).await;
    for settings in [
        r#"{"interests":" \n "}"#,
        r#"{"interests":"Rust","autoRefresh":{"mode":"daily","time":"25:00"}}"#,
    ] {
        let (status, body) =
            send_json(app.clone(), "PUT", &settings_uri(id), &cookie, settings).await;
        assert_error(
            status,
            &body,
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request_body",
        );
    }

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

/// 残りを使うので、AI を使えない編集者は集め直せない。ほかの人が作ったグループの設定も、表示しないものも変えられない。
#[sqlx::test]
async fn an_editor_without_ai_cannot_refresh_or_change_the_setting(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with_interests(&app, &pool, &cookie).await;
    prepare(&fake, SEARCH_BODY);
    refreshed(&app, id, &cookie).await;
    insert_user(&pool, "taro", "password123").await;
    let taro = login(app.clone(), "taro", "password123").await;

    let seen = get_json(app.clone(), &links_uri(id), &taro).await;
    assert_eq!(seen["canRefresh"], false, "{seen}");
    assert_eq!(seen["canEdit"], false, "{seen}");
    let (status, body) = refresh(&app, id, &taro).await;
    assert_error(status, &body, StatusCode::FORBIDDEN, "ai_not_allowed");
    assert_eq!(fake.lock().expect("鍵が毒された").searches.len(), 2);

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &settings_uri(id),
        &taro,
        r#"{"interests":"Rust"}"#,
    )
    .await;
    assert_error(status, &body, StatusCode::FORBIDDEN, "forbidden");
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &exclusions_uri(id),
        &taro,
        r#"{"url":"https://a.example/","kind":"page"}"#,
    )
    .await;
    assert_error(status, &body, StatusCode::FORBIDDEN, "forbidden");
}

/// AI の答えにクエリが無ければ、前に集めたリンクを残して 502 にする。
#[sqlx::test]
async fn an_answer_without_queries_keeps_the_previous_links(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with_interests(&app, &pool, &cookie).await;
    prepare(&fake, SEARCH_BODY);
    refreshed(&app, id, &cookie).await;

    // 検索キーワードを変えて、クエリを作り直させる。
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &settings_uri(id),
        &cookie,
        r#"{"interests":"トマト"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    fake.lock().expect("鍵が毒された").ai = (StatusCode::OK, answer("作れませんでした"), Some(5));
    let (status, body) = refresh(&app, id, &cookie).await;
    assert_error(status, &body, StatusCode::BAD_GATEWAY, "ai_upstream");
    let seen = get_json(app.clone(), &links_uri(id), &cookie).await;
    assert_eq!(seen["links"].as_array().map(Vec::len), Some(3), "{seen}");
}

/// カードの取り直しは、見られるグループの集めたリンクだけを対象にする。
#[sqlx::test]
async fn link_previews_are_refreshed_for_a_viewable_group_only(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with_interests(&app, &pool, &cookie).await;
    prepare(&fake, SEARCH_BODY);
    refreshed(&app, id, &cookie).await;
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
