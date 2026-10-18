//! AI が集めるリンク: グループの設定・集め直し・自動の更新・表示しない (→ docs/ai.md「AI が集めるリンク」)。
//! 窓口は `ai.rs` の偽のサーバーに差し替える。検索キーワード・自動の更新・候補の外し方の場合分けは `src/api/ai_links.rs` の単体テスト。

use super::ai::{AiApp, ai_app, get_json, usage_rows};
use super::*;

/// 検索の候補 3 件。2件目は、グループに登録してあるリンク (`insert_registered` の URL) と同じ。
/// どれも一覧のページの形の URL なので開こうとするが、`.invalid` は名前解決できないと決まっている (RFC 6761) ので外へ出ない。
const SEARCH_BODY: &str = r#"{"results":[
    {"title":"C","url":"https://c.invalid/","snippet":"about c"},
    {"title":"Registered","url":"https://registered.invalid/","snippet":""},
    {"title":"A","url":"https://a.invalid/","date":"2026-01-02","snippet":"about a"}
]}"#;

/// `SEARCH_BODY` の A に加えて、新しいページ D が出た検索の結果。C は出なくなった。
const SEARCH_BODY_WITH_D: &str = r#"{"results":[
    {"title":"A","url":"https://a.invalid/","date":"2026-01-02","snippet":"about a"},
    {"title":"D","url":"https://d.invalid/","snippet":"about d"}
]}"#;

/// AI の答え。`text` を文として返す、Gemini の Interactions API の形。
fn answer(text: &str) -> String {
    serde_json::json!({
        "steps": [{ "type": "model_output", "content": [{ "type": "text", "text": text }] }]
    })
    .to_string()
}

/// グループに、検索の候補と同じ URL のリンクを登録する。
async fn insert_registered(pool: &SqlitePool, title: &str, group: i64) {
    sqlx::query(
        "INSERT INTO contents (type, parent_id, title, url, visibility) \
         VALUES ('link', ?, ?, 'https://registered.invalid/', 'public')",
    )
    .bind(group)
    .bind(title)
    .execute(pool)
    .await
    .expect("リンクを登録できなかった");
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

/// 検索の答えを用意する。AI は、まとめる記事が2件以上あるか、一覧のページから記事を取り出せたときだけ呼ばれる。
fn prepare(fake: &super::ai::SharedFake, search: &str) {
    let mut fake = fake.lock().expect("鍵が毒された");
    fake.search = (StatusCode::OK, search.to_string(), Some(10));
    fake.ai = (StatusCode::OK, answer("[]"), Some(5));
}

fn set_search(fake: &super::ai::SharedFake, search: &str) {
    fake.lock().expect("鍵が毒された").search = (StatusCode::OK, search.to_string(), Some(10));
}

/// 公開のグループを作り、`settings` (JSON) で集める設定を付ける。
async fn group_with(app: &Router, pool: &SqlitePool, cookie: &str, settings: &str) -> i64 {
    let id = insert_group_under(pool, "集める", "public", None).await;
    put_settings(app, id, cookie, settings).await;
    id
}

async fn put_settings(app: &Router, id: i64, cookie: &str, settings: &str) {
    let (status, body) = send_json(app.clone(), "PUT", &settings_uri(id), cookie, settings).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn group_with_interests(app: &Router, pool: &SqlitePool, cookie: &str) -> i64 {
    group_with(
        app,
        pool,
        cookie,
        r#"{"interests":"Rust 非同期\n家庭菜園"}"#,
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

/// 設定 → 集め直す → 匿名で見る、の一巡。検索キーワードの行をそのままクエリにして、検索は2回。AI は呼ばない。
/// 登録済みの URL は並べない。2回目は、新しく出たページを上に足して、出なくなったページを外す。
#[sqlx::test]
async fn refreshing_searches_the_lines_as_written_and_adds_new_results_on_top(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with_interests(&app, &pool, &cookie).await;
    insert_registered(&pool, "登録済み", id).await;
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
            ("https://a.invalid/".to_string(), false),
            ("https://c.invalid/".to_string(), false)
        ],
        "{first}"
    );
    assert_eq!(first["added"], 2, "{first}");
    assert_eq!(first["links"][1]["title"], "C", "{first}");
    {
        let fake = fake.lock().expect("鍵が毒された");
        assert_eq!(fake.searches.len(), 1, "{:?}", fake.searches);
        assert_eq!(
            fake.searches[0],
            serde_json::json!({
                "queries": ["Rust 非同期", "家庭菜園"], "country": "JP", "language": "ja"
            })
        );
        assert!(fake.prompts.is_empty(), "{:?}", fake.prompts);
    }

    // 見つけた時刻は秒の単位なので、1回目と分かれるように待つ。
    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
    set_search(&fake, SEARCH_BODY_WITH_D);
    let second = refreshed(&app, id, &cookie).await;
    assert_eq!(
        shown(&second),
        [
            ("https://d.invalid/".to_string(), true),
            ("https://a.invalid/".to_string(), false)
        ],
        "{second}"
    );
    assert_eq!(second["added"], 1, "{second}");

    // 新しいものが無かった集め直しでは、前に足した分の印を消さない。
    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
    let third = refreshed(&app, id, &cookie).await;
    assert_eq!(third["added"], 0, "{third}");
    assert_eq!(shown(&third), shown(&second), "{third}");

    let kinds: Vec<(String, i64)> = usage_rows(&pool)
        .await
        .into_iter()
        .map(|(_, _, kind, used)| (kind, used))
        .collect();
    assert_eq!(kinds, [10; 3].map(|used| ("links".to_string(), used)));

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
        r#"{"results":[{"title":"A","url":"https://a.invalid/","snippet":""}]}"#,
    );
    let again = refreshed(&app, id, &cookie).await;
    assert_eq!(again["added"], 0, "{again}");
    assert_eq!(
        urls(&again),
        [
            "https://a.invalid/",
            "https://c.invalid/",
            "https://registered.invalid/"
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
        urls(&collected).contains(&"https://registered.invalid/".to_string()),
        "{collected}"
    );

    insert_registered(&pool, "登録した", id).await;
    let seen = get_json(app.clone(), &links_uri(id), &cookie).await;
    assert_eq!(
        urls(&seen),
        ["https://a.invalid/", "https://c.invalid/"],
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
    assert_eq!(fake.lock().expect("鍵が毒された").searches.len(), 1);
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
    assert_eq!(fake.lock().expect("鍵が毒された").searches.len(), 1);
}

/// 自動の更新が失敗したら、しばらくは開かれても試さない (障害の間に残りを使い続けないため)。
#[sqlx::test]
async fn a_failed_auto_refresh_is_not_retried_on_every_open(pool: SqlitePool) {
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
    fake.lock().expect("鍵が毒された").search = (StatusCode::BAD_GATEWAY, "{}".to_string(), None);

    let (status, body) = send_anon_json(app.clone(), "POST", &auto_refresh_uri(id), "{}").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""autoRefreshDue":false"#), "{body}");
    let searched = fake.lock().expect("鍵が毒された").searches.len();
    assert!(searched > 0, "1回目は試すはず");

    let (status, body) = send_anon_json(app.clone(), "POST", &auto_refresh_uri(id), "{}").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(fake.lock().expect("鍵が毒された").searches.len(), searched);
}

/// 表示しないにしたページとサイトは並ばず、集め直しても出ない。取り消すと、集め直さなくてもまた並ぶ。
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
        r#"{"url":"https://a.invalid/","kind":"page"}"#,
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
        r#"{"url":"https://c.invalid/","kind":"site"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains(r#""value":"c.invalid""#), "{body}");

    let seen = get_json(app.clone(), &links_uri(id), &cookie).await;
    assert_eq!(urls(&seen), ["https://registered.invalid/"], "{seen}");
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
            {"title":"C2","url":"https://www.c.invalid/2","snippet":""},
            {"title":"E","url":"https://e.invalid/","snippet":""},
            {"title":"A","url":"https://a.invalid/","snippet":""}
        ]}"#,
    );
    let again = refreshed(&app, id, &cookie).await;
    assert_eq!(again["added"], 1, "{again}");
    assert_eq!(urls(&again), ["https://e.invalid/"], "{again}");

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
    let seen = get_json(app.clone(), &links_uri(id), &cookie).await;
    assert_eq!(
        urls(&seen),
        ["https://e.invalid/", "https://a.invalid/"],
        "{seen}"
    );
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

/// オフにすると並ばず、集め直しは 404 になる。設定と表示しないものは残り、オンに戻すと前のとおりに並ぶ。
#[sqlx::test]
async fn turning_the_setting_off_keeps_the_settings_for_turning_it_on_again(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with(
        &app,
        &pool,
        &cookie,
        r#"{"interests":"Rust","dailyLimit":3}"#,
    )
    .await;
    prepare(&fake, SEARCH_BODY);
    refreshed(&app, id, &cookie).await;
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &exclusions_uri(id),
        &cookie,
        r#"{"url":"https://c.invalid/","kind":"page"}"#,
    )
    .await;
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
    let settings: serde_json::Value = serde_json::from_str(&body).expect("JSON でない");
    assert_eq!(settings["enabled"], false, "{settings}");
    assert_eq!(settings["interests"], "Rust", "{settings}");
    assert_eq!(settings["dailyLimit"], 3, "{settings}");
    assert_eq!(
        settings["exclusions"].as_array().map(Vec::len),
        Some(1),
        "{settings}"
    );
    let seen = get_json(app.clone(), &links_uri(id), &cookie).await;
    assert_eq!(seen["enabled"], false, "{seen}");
    assert_eq!(seen["links"].as_array().map(Vec::len), Some(0), "{seen}");
    let (status, body) = refresh(&app, id, &cookie).await;
    assert_error(status, &body, StatusCode::NOT_FOUND, "not_found");

    put_settings(&app, id, &cookie, r#"{"interests":"Rust","dailyLimit":3}"#).await;
    let seen = get_json(app.clone(), &links_uri(id), &cookie).await;
    assert_eq!(
        urls(&seen),
        ["https://a.invalid/", "https://registered.invalid/"],
        "{seen}"
    );
}

/// 検索キーワードを変えると、次に集めるときから、変えた行で検索する。
#[sqlx::test]
async fn changing_the_interests_changes_the_queries(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with_interests(&app, &pool, &cookie).await;
    prepare(&fake, SEARCH_BODY);
    refreshed(&app, id, &cookie).await;
    put_settings(
        &app,
        id,
        &cookie,
        r#"{"interests":"トマトの育て方","dailyLimit":3}"#,
    )
    .await;
    refreshed(&app, id, &cookie).await;
    let searches = fake.lock().expect("鍵が毒された").searches.clone();
    assert_eq!(searches.len(), 2, "{searches:?}");
    assert_eq!(
        searches[1]["queries"],
        serde_json::json!(["トマトの育て方"])
    );
}

/// 日数を決めると、PC の今日からその日数前より後に公開されたページに絞って検索する。
#[sqlx::test]
async fn within_days_narrows_the_search_to_pages_published_after_the_day(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with(
        &app,
        &pool,
        &cookie,
        r#"{"interests":"Rust","withinDays":7}"#,
    )
    .await;
    prepare(&fake, SEARCH_BODY);
    refreshed(&app, id, &cookie).await;
    let searches = fake.lock().expect("鍵が毒された").searches.clone();
    let week_ago: String = sqlx::query_scalar("SELECT date('now', 'localtime', '-7 days')")
        .fetch_one(&pool)
        .await
        .expect("日付を出せなかった");
    assert_eq!(searches[0]["after"], week_ago.as_str(), "{searches:?}");
    let settings = get_json(app.clone(), &settings_uri(id), &cookie).await;
    assert_eq!(settings["withinDays"], 7, "{settings}");
}

#[sqlx::test]
async fn settings_reject_bad_values_and_non_groups(pool: SqlitePool) {
    let AiApp { app, cookie, .. } = ai_app(&pool, true).await;
    let id = insert_group_under(&pool, "集める", "public", None).await;
    for settings in [
        r#"{"interests":" \n "}"#,
        r#"{"interests":"1\n2\n3\n4\n5\n6"}"#,
        r#"{"interests":"Rust","withinDays":-1}"#,
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
    assert_eq!(fake.lock().expect("鍵が毒された").searches.len(), 1);

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
        r#"{"url":"https://a.invalid/","kind":"page"}"#,
    )
    .await;
    assert_error(status, &body, StatusCode::FORBIDDEN, "forbidden");
}

/// カードを取り直して無くなっていたと分かったページは並べず、集め直しで出ても足した数に入れない。
#[sqlx::test]
async fn pages_found_gone_are_hidden_and_not_counted_as_added(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with_interests(&app, &pool, &cookie).await;
    prepare(&fake, SEARCH_BODY);
    refreshed(&app, id, &cookie).await;
    for url in ["https://a.invalid/", "https://d.invalid/"] {
        insert_fresh_preview(&pool, url, None).await;
    }
    sqlx::query("UPDATE link_previews SET gone = 1")
        .execute(&pool)
        .await
        .expect("無くなっていた印を付けられなかった");

    let seen = get_json(app.clone(), &links_uri(id), &cookie).await;
    assert_eq!(
        urls(&seen),
        ["https://c.invalid/", "https://registered.invalid/"],
        "{seen}"
    );

    set_search(&fake, SEARCH_BODY_WITH_D);
    let again = refreshed(&app, id, &cookie).await;
    assert_eq!(again["added"], 0, "{again}");
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
    insert_fresh_preview(&pool, "https://a.invalid/", None).await;

    let request = format!(r#"{{"contentIds":[],"aiLinksGroup":{id}}}"#);
    let (status, body) = send_anon_json(
        app.clone(),
        "POST",
        "/api/v1/link-previews/refresh",
        &request,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("https://a.invalid/"), "{body}");

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

/// 個別の記事を AI に話題ごとにまとめさせ、扱うサイトの多い話題から、話題ごとに1件を並べる。
/// 一覧のページ (分類のトップなど) は、開けなくても外さず、個別の記事の後ろに並ぶ。
#[sqlx::test]
async fn refreshing_shows_one_article_per_story_with_stories_on_more_sites_first(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let id = group_with(&app, &pool, &cookie, r#"{"interests":"千葉県のニュース"}"#).await;
    prepare(
        &fake,
        r#"{"results":[
            {"title":"ニュース一覧","url":"https://news.invalid/news/","date":"2026-10-18","snippet":""},
            {"title":"別の話題","url":"https://c.invalid/articles/3","snippet":""},
            {"title":"知事が会見","url":"https://a.invalid/articles/1","snippet":""},
            {"title":"知事が記者会見","url":"https://b.invalid/articles/2","snippet":""}
        ]}"#,
    );
    fake.lock().expect("鍵が毒された").ai = (StatusCode::OK, answer("[[1], [2, 3]]"), Some(5));
    let links = refreshed(&app, id, &cookie).await;
    assert_eq!(
        urls(&links),
        [
            "https://a.invalid/articles/1",
            "https://c.invalid/articles/3",
            "https://news.invalid/news/"
        ],
        "{links}"
    );
    let prompts = fake.lock().expect("鍵が毒された").prompts.clone();
    assert_eq!(prompts.len(), 1, "{prompts:?}");
    let asked = prompts[0]["user"].as_str().expect("文が無い");
    assert!(asked.contains("2. [a.invalid] 知事が会見"), "{asked}");
}
