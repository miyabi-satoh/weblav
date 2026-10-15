//! AI の機能の土台: PC の登録・誰が使えるか・窓口の呼び出しと使った量の記録 (→ docs/ai.md)。
//! 窓口は、決まった応答を返す偽のサーバーに差し替える。

use std::sync::{Arc, Mutex};

use axum::Json;
use axum::http::HeaderMap;
use axum::routing::{delete, get, post};
use sha2::{Digest, Sha256};

use super::*;

const TOKEN: &str = "app-token";

/// AI の答え。Gemini の Interactions API の形。
const ANSWER_BODY: &str =
    r#"{"steps":[{"type":"model_output","content":[{"type":"text","text":"{\"axes\":[]}"}]}]}"#;

/// 偽の窓口の振る舞いと、受け取ったもの。
pub(super) struct Fake {
    /// 窓口の画面で登録を認めたか。
    approved: bool,
    /// 窓口の側で登録を外したか (トークンを断る)。
    revoked: bool,
    /// `/v1/ai` の応答 (ステータス・本文・使った量)。
    pub(super) ai: (StatusCode, String, Option<i64>),
    /// 呼ばれた順に返す `/v1/ai` の応答。空なら `ai` を返す。1回の操作で AI を何度も呼ぶ機能のため。
    pub(super) ai_queue: std::collections::VecDeque<(StatusCode, String, Option<i64>)>,
    /// `/v1/search` の応答 (ステータス・本文・使った量)。
    pub(super) search: (StatusCode, String, Option<i64>),
    /// `/v1/links/token` に届いた検証用の値。
    verifiers: Vec<String>,
    /// `/v1/ai` に届いた本文。
    pub(super) prompts: Vec<serde_json::Value>,
    /// `/v1/search` に届いた本文。
    pub(super) searches: Vec<serde_json::Value>,
    /// `DELETE /v1/token` が届いたか。
    token_deleted: bool,
}

type SharedFake = Arc<Mutex<Fake>>;

fn authorized(fake: &Fake, headers: &HeaderMap) -> bool {
    !fake.revoked
        && headers
            .get(header::AUTHORIZATION)
            .is_some_and(|value| value.as_bytes() == format!("Bearer {TOKEN}").as_bytes())
}

fn unauthorized() -> Response {
    axum::response::IntoResponse::into_response((
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "unauthorized" })),
    ))
}

/// 窓口の代わりに、`Fake` のとおりに応えるサーバーを立て、その URL を返す。
async fn fake_account_server() -> (String, SharedFake) {
    use axum::extract::State;
    use axum::response::IntoResponse;

    let fake = Arc::new(Mutex::new(Fake {
        approved: false,
        revoked: false,
        ai: (StatusCode::OK, ANSWER_BODY.to_string(), Some(1234)),
        ai_queue: std::collections::VecDeque::new(),
        search: (StatusCode::OK, r#"{"results":[]}"#.to_string(), Some(10)),
        verifiers: Vec::new(),
        prompts: Vec::new(),
        searches: Vec::new(),
        token_deleted: false,
    }));
    let router = Router::new()
        .route(
            "/v1/links/token",
            post(
                |State(fake): State<SharedFake>, Json(body): Json<serde_json::Value>| async move {
                    let mut fake = fake.lock().expect("偽の窓口の鍵が毒された");
                    fake.verifiers.push(
                        body["code_verifier"]
                            .as_str()
                            .expect("検証用の値が届かなかった")
                            .to_string(),
                    );
                    if fake.approved {
                        Json(serde_json::json!({ "token": TOKEN })).into_response()
                    } else {
                        (
                            StatusCode::ACCEPTED,
                            Json(serde_json::json!({ "status": "pending" })),
                        )
                            .into_response()
                    }
                },
            ),
        )
        .route(
            "/v1/balance",
            get(
                |State(fake): State<SharedFake>, headers: HeaderMap| async move {
                    if !authorized(&fake.lock().expect("偽の窓口の鍵が毒された"), &headers) {
                        return unauthorized();
                    }
                    Json(serde_json::json!({ "email": "owner@example.com", "remaining_percent": 42 }))
                        .into_response()
                },
            ),
        )
        .route(
            "/v1/token",
            delete(|State(fake): State<SharedFake>| async move {
                fake.lock().expect("偽の窓口の鍵が毒された").token_deleted = true;
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/v1/ai",
            post(
                |State(fake): State<SharedFake>,
                 headers: HeaderMap,
                 Json(body): Json<serde_json::Value>| async move {
                    let mut fake = fake.lock().expect("偽の窓口の鍵が毒された");
                    if !authorized(&fake, &headers) {
                        return unauthorized();
                    }
                    fake.prompts.push(body);
                    let (status, body, used) =
                        fake.ai_queue.pop_front().unwrap_or_else(|| fake.ai.clone());
                    let mut response = (status, body).into_response();
                    if let Some(used) = used {
                        response
                            .headers_mut()
                            .insert("x-weblav-used", used.into());
                    }
                    response
                },
            ),
        )
        .route(
            "/v1/search",
            post(
                |State(fake): State<SharedFake>,
                 headers: HeaderMap,
                 Json(body): Json<serde_json::Value>| async move {
                    let mut fake = fake.lock().expect("偽の窓口の鍵が毒された");
                    if !authorized(&fake, &headers) {
                        return unauthorized();
                    }
                    fake.searches.push(body);
                    let (status, body, used) = fake.search.clone();
                    let mut response = (status, body).into_response();
                    if let Some(used) = used {
                        response
                            .headers_mut()
                            .insert("x-weblav-used", used.into());
                    }
                    response
                },
            ),
        )
        .with_state(Arc::clone(&fake));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("偽の窓口のポートを取れなかった");
    let addr = listener
        .local_addr()
        .expect("偽の窓口のアドレスを読めなかった");
    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("偽の窓口が落ちた")
    });
    (format!("http://{addr}"), fake)
}

pub(super) struct AiApp {
    pub(super) app: Router,
    /// 管理者のセッション。
    pub(super) cookie: String,
    pub(super) fake: SharedFake,
    server: String,
    /// 登録の状態のファイル。
    account_path: std::path::PathBuf,
}

/// `server` を窓口にしたアプリを、データの置き場 `dir` で組み立てる。
async fn app_with_account_server(pool: SqlitePool, dir: &std::path::Path, server: &str) -> Router {
    build_app(
        pool,
        Key::generate(),
        &Config::default(),
        &AppDirs {
            config_dir: dir.to_path_buf(),
            data_dir: dir.to_path_buf(),
        },
        None,
        FolderPicker::new(|| Ok(None)),
        server,
    )
    .await
    .expect("failed to build app")
}

/// 登録済みの状態のファイルを置いた、データの置き場を作る。
fn linked_data_dir() -> std::path::PathBuf {
    let dir = next_app_dir();
    std::fs::create_dir_all(&dir).expect("データの置き場を作れなかった");
    std::fs::write(
        dir.join("ai-account.json"),
        format!(r#"{{"state":"linked","token":"{TOKEN}"}}"#),
    )
    .expect("登録の状態を書けなかった");
    dir
}

/// 偽の窓口につなぐアプリと、管理者のセッション。`linked` なら登録済みから始める。
pub(super) async fn ai_app(pool: &SqlitePool, linked: bool) -> AiApp {
    let (server, fake) = fake_account_server().await;
    let dir = if linked {
        linked_data_dir()
    } else {
        next_app_dir()
    };
    register_content_root(pool).await;
    let app = app_with_account_server(pool.clone(), &dir, &server).await;
    let cookie = admin_cookie(app.clone(), pool).await;
    AiApp {
        app,
        cookie,
        fake,
        server,
        account_path: dir.join("ai-account.json"),
    }
}

pub(super) async fn get_json(app: Router, uri: &str, cookie: &str) -> serde_json::Value {
    let (status, body) = send_empty(app, "GET", uri, cookie).await;
    assert_eq!(status, StatusCode::OK, "{uri}: {body}");
    serde_json::from_str(&body).expect("応答を JSON として読めなかった")
}

const STATUS_URI: &str = "/api/v1/admin/ai";
const LINK_URI: &str = "/api/v1/admin/ai/link";
const POLL_URI: &str = "/api/v1/admin/ai/link/poll";
const USAGE_URI: &str = "/api/v1/admin/ai/usage";
const AVAILABILITY_URI: &str = "/api/v1/ai/availability";

/// アーカイブを1つ作り、軸の定義を AI に頼む口の URI を返す。
async fn generate_uri(app: Router, cookie: &str, name: &str) -> (String, test_support::TempDir) {
    let dir = temp_test_dir(name);
    let id = create_archive(app, cookie, &dir, None).await;
    (format!("/api/v1/contents/{id}/axes/ai/generate"), dir)
}

async fn generate(app: Router, uri: &str, cookie: &str) -> (StatusCode, String) {
    send_json(app, "POST", uri, cookie, r#"{"prompt":"フォルダーの一覧"}"#).await
}

pub(super) async fn usage_rows(pool: &SqlitePool) -> Vec<(Option<i64>, String, String, i64)> {
    sqlx::query_as("SELECT user_id, username, kind, used FROM ai_usage ORDER BY id")
        .fetch_all(pool)
        .await
        .expect("使った量の記録を読めなかった")
}

/// 始める → まだ認めていない → 認めた後に登録済み → 解除、の一巡。
#[sqlx::test]
async fn linking_goes_from_pending_to_linked_and_back(pool: SqlitePool) {
    let AiApp {
        app,
        cookie,
        fake,
        server,
        account_path,
    } = ai_app(&pool, false).await;

    let status = get_json(app.clone(), STATUS_URI, &cookie).await;
    assert_eq!(status["state"], "unlinked", "{status}");
    assert_eq!(status["accountUrl"], format!("{server}/account/"));
    assert!(status["linkUrl"].is_null(), "{status}");

    let (code, body) = send_empty(app.clone(), "POST", LINK_URI, &cookie).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let link: serde_json::Value = serde_json::from_str(&body).expect("応答を読めなかった");
    assert_eq!(link["state"], "pending", "{body}");
    let link_url = link["linkUrl"].as_str().expect("URL が返らなかった");
    assert_eq!(
        get_json(app.clone(), STATUS_URI, &cookie).await["linkUrl"],
        link_url,
        "状態の口も同じ URL を返す"
    );

    let (code, body) = send_empty(app.clone(), "POST", POLL_URI, &cookie).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(json_str(&body, "state"), "pending", "{body}");

    // URL に載るのは検証用の値のハッシュで、窓口へ送るのは値そのもの。
    let verifier = fake.lock().expect("鍵が毒された").verifiers[0].clone();
    let challenge: String = Sha256::digest(verifier.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert!(
        link_url.starts_with(&format!(
            "{server}/account/link?challenge={challenge}&name="
        )),
        "{link_url}"
    );
    assert!(!link_url.contains(&verifier), "{link_url}");
    assert_eq!(
        link["confirmationCode"],
        challenge[..6].to_uppercase(),
        "窓口の画面と同じ確認コードを返す"
    );

    fake.lock().expect("鍵が毒された").approved = true;
    let (code, body) = send_empty(app.clone(), "POST", POLL_URI, &cookie).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(json_str(&body, "state"), "linked", "{body}");
    let file = std::fs::read_to_string(&account_path).expect("登録の状態のファイルが無い");
    assert!(file.contains(TOKEN), "{file}");

    let status = get_json(app.clone(), STATUS_URI, &cookie).await;
    assert_eq!(status["state"], "linked", "{status}");
    assert_eq!(status["email"], "owner@example.com", "{status}");
    assert_eq!(status["remainingPercent"], 42.0, "{status}");
    assert_eq!(status["accountUnreachable"], false, "{status}");

    let (code, body) = send_empty(app.clone(), "POST", LINK_URI, &cookie).await;
    assert_error(code, &body, StatusCode::CONFLICT, "conflict");

    let (code, body) = send_empty(app.clone(), "DELETE", STATUS_URI, &cookie).await;
    assert_eq!(code, StatusCode::NO_CONTENT, "{body}");
    assert!(fake.lock().expect("鍵が毒された").token_deleted);
    assert!(!account_path.exists(), "解除したらトークンを残さない");
    let status = get_json(app, STATUS_URI, &cookie).await;
    assert_eq!(status["state"], "unlinked", "{status}");
}

/// 期限の決まりは `src/ai.rs` の単体テストで確かめる。ここでは、状態の口につながっていることだけを見る。
#[sqlx::test]
async fn an_expired_pending_link_is_reported_as_unlinked(pool: SqlitePool) {
    let dir = next_app_dir();
    std::fs::create_dir_all(&dir).expect("データの置き場を作れなかった");
    let account_path = dir.join("ai-account.json");
    // 始めた時刻が 0 (1970 年) なら、期限は必ず過ぎている。
    std::fs::write(
        &account_path,
        r#"{"state":"pending","code_verifier":"old","started_at":0}"#,
    )
    .expect("登録の状態を書けなかった");
    let app = app_with_account_server(pool.clone(), &dir, NO_ACCOUNT_SERVER).await;
    let cookie = admin_cookie(app.clone(), &pool).await;

    let status = get_json(app, STATUS_URI, &cookie).await;
    assert_eq!(status["state"], "unlinked", "{status}");
    assert!(status["linkUrl"].is_null(), "{status}");
    assert!(!account_path.exists(), "期限の切れた検証用の値を残さない");
}

#[sqlx::test]
async fn cancelling_a_pending_link_returns_to_unlinked(pool: SqlitePool) {
    let AiApp {
        app,
        cookie,
        account_path,
        ..
    } = ai_app(&pool, false).await;

    let (code, body) = send_empty(app.clone(), "POST", LINK_URI, &cookie).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert!(account_path.exists());

    let (code, body) = send_empty(app.clone(), "DELETE", LINK_URI, &cookie).await;
    assert_eq!(code, StatusCode::NO_CONTENT, "{body}");
    assert!(!account_path.exists());
    let (code, body) = send_empty(app, "POST", POLL_URI, &cookie).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(json_str(&body, "state"), "unlinked", "{body}");
}

/// 窓口の側で登録を外されていたら、手元のトークンを消して未登録に戻す。
#[sqlx::test]
async fn a_token_rejected_by_the_account_server_is_forgotten(pool: SqlitePool) {
    let AiApp {
        app,
        cookie,
        fake,
        account_path,
        ..
    } = ai_app(&pool, true).await;
    fake.lock().expect("鍵が毒された").revoked = true;

    let status = get_json(app.clone(), STATUS_URI, &cookie).await;
    assert_eq!(status["state"], "unlinked", "{status}");
    assert_eq!(status["accountUnreachable"], false, "{status}");
    assert!(!account_path.exists());
}

/// 窓口につながらなくても登録済みのままで、残りだけが分からない。
#[sqlx::test]
async fn status_stays_linked_when_the_account_server_is_unreachable(pool: SqlitePool) {
    let dir = linked_data_dir();
    register_content_root(&pool).await;
    let app = app_with_account_server(pool.clone(), &dir, NO_ACCOUNT_SERVER).await;
    let cookie = admin_cookie(app.clone(), &pool).await;

    let status = get_json(app.clone(), STATUS_URI, &cookie).await;
    assert_eq!(status["state"], "linked", "{status}");
    assert!(status["remainingPercent"].is_null(), "{status}");
    assert!(status["email"].is_null(), "{status}");
    assert_eq!(status["accountUnreachable"], true, "{status}");

    let (uri, _dir) = generate_uri(app.clone(), &cookie, "ai-unreachable").await;
    let (code, body) = generate(app.clone(), &uri, &cookie).await;
    assert_error(
        code,
        &body,
        StatusCode::BAD_GATEWAY,
        "account_server_unavailable",
    );

    // 解除は、窓口につながらなくても手元のトークンを消す。
    let (code, body) = send_empty(app, "DELETE", STATUS_URI, &cookie).await;
    assert_eq!(code, StatusCode::NO_CONTENT, "{body}");
    assert!(!dir.join("ai-account.json").exists());
}

#[sqlx::test]
async fn generating_axes_returns_the_answer_and_records_the_usage(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let (uri, _dir) = generate_uri(app.clone(), &cookie, "ai-generate").await;

    let (code, body) = generate(app.clone(), &uri, &cookie).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(json_str(&body, "answer"), r#"{"axes":[]}"#);
    assert_eq!(
        fake.lock().expect("鍵が毒された").prompts,
        [serde_json::json!({ "user": "フォルダーの一覧" })]
    );

    let admin_id = db_user_id(&pool, "admin-alice").await;
    assert_eq!(
        usage_rows(&pool).await,
        [(
            Some(admin_id),
            "admin-alice".to_string(),
            "axes".to_string(),
            1234
        )]
    );

    generate(app.clone(), &uri, &cookie).await;
    let usage = get_json(app, USAGE_URI, &cookie).await;
    assert_eq!(usage.as_array().map(Vec::len), Some(1), "{usage}");
    assert_eq!(usage[0]["userId"], admin_id, "{usage}");
    assert_eq!(usage[0]["username"], "admin-alice", "{usage}");
    assert_eq!(usage[0]["count"], 2, "{usage}");
    assert_eq!(usage[0]["used"], 2468, "{usage}");
    assert!(usage[0]["lastUsedAt"].is_string(), "{usage}");
}

/// 窓口の断り方ごとに、画面へ返す code を分ける。
#[sqlx::test]
async fn account_server_failures_map_to_their_own_codes(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;
    let (uri, _dir) = generate_uri(app.clone(), &cookie, "ai-failures").await;

    let cases = [
        (402, r#"{"error":"no_credit"}"#, 402, "ai_no_credit"),
        (429, r#"{"error":"rate_limited"}"#, 429, "ai_rate_limited"),
        (413, r#"{"error":"too_long"}"#, 413, "ai_too_large"),
        (502, r#"{"error":"upstream"}"#, 502, "ai_upstream"),
        // 窓口の手前が返す 502 や、窓口の不具合は、つながらないのと同じに扱う。
        (502, "Bad Gateway", 502, "account_server_unavailable"),
        (500, "", 502, "account_server_unavailable"),
        // 答えの文が無い返事。
        (200, r#"{"steps":[]}"#, 502, "ai_upstream"),
    ];
    for (fake_status, fake_body, expected_status, expected_code) in cases {
        fake.lock().expect("鍵が毒された").ai = (
            StatusCode::from_u16(fake_status).expect("ステータスが不正"),
            fake_body.to_string(),
            None,
        );
        let (code, body) = generate(app.clone(), &uri, &cookie).await;
        assert_error_case(
            code,
            &body,
            StatusCode::from_u16(expected_status).expect("ステータスが不正"),
            expected_code,
            &format!("{fake_status} {fake_body}"),
        );
    }
    assert_eq!(
        usage_rows(&pool).await,
        [],
        "使った量が返らなければ記録しない"
    );

    // 失敗でも、AI が使った分を窓口が返していれば記録する。
    fake.lock().expect("鍵が毒された").ai = (
        StatusCode::BAD_GATEWAY,
        r#"{"error":"upstream"}"#.to_string(),
        Some(77),
    );
    let (code, body) = generate(app.clone(), &uri, &cookie).await;
    assert_error(code, &body, StatusCode::BAD_GATEWAY, "ai_upstream");
    assert_eq!(usage_rows(&pool).await.len(), 1);
    assert_eq!(usage_rows(&pool).await[0].3, 77);

    // 窓口の上限を超える長さは、窓口へ送らずに断る。
    let sent = fake.lock().expect("鍵が毒された").prompts.len();
    let long = format!(r#"{{"prompt":"{}"}}"#, "a".repeat(200_001));
    let (code, body) = send_json(app.clone(), "POST", &uri, &cookie, &long).await;
    assert_error(code, &body, StatusCode::PAYLOAD_TOO_LARGE, "ai_too_large");
    assert_eq!(fake.lock().expect("鍵が毒された").prompts.len(), sent);

    // トークンを断られたら未登録に戻り、次からは窓口を呼ばない。
    fake.lock().expect("鍵が毒された").revoked = true;
    let (code, body) = generate(app.clone(), &uri, &cookie).await;
    assert_error(code, &body, StatusCode::CONFLICT, "ai_not_linked");
    let availability = get_json(app.clone(), AVAILABILITY_URI, &cookie).await;
    assert_eq!(availability["available"], false, "{availability}");
    let (code, body) = generate(app, &uri, &cookie).await;
    assert_error(code, &body, StatusCode::CONFLICT, "ai_not_linked");
}

/// 編集者は、管理者が印を付けるまで使えない。管理者は印が無くても使える。
#[sqlx::test]
async fn editors_need_the_ai_allowed_flag(pool: SqlitePool) {
    insert_user(&pool, "taro", "password").await;
    let taro_id = db_user_id(&pool, "taro").await;
    let AiApp {
        app,
        cookie: admin,
        fake,
        ..
    } = ai_app(&pool, true).await;
    let taro = login(app.clone(), "taro", "password").await;
    let (uri, _dir) = generate_uri(app.clone(), &taro, "ai-editor").await;

    let users = get_json(app.clone(), "/api/v1/admin/users", &admin).await;
    assert!(
        users
            .as_array()
            .expect("配列でない")
            .iter()
            .all(|user| user["aiAllowed"] == false),
        "{users}"
    );
    let availability = get_json(app.clone(), AVAILABILITY_URI, &taro).await;
    assert_eq!(availability["available"], false, "{availability}");
    let availability = get_json(app.clone(), AVAILABILITY_URI, &admin).await;
    assert_eq!(availability["available"], true, "{availability}");

    let (code, body) = generate(app.clone(), &uri, &taro).await;
    assert_error(code, &body, StatusCode::FORBIDDEN, "ai_not_allowed");
    assert!(fake.lock().expect("鍵が毒された").prompts.is_empty());

    let (code, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/v1/admin/users/{taro_id}/ai-allowed"),
        &admin,
        r#"{"aiAllowed":true}"#,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let updated: serde_json::Value = serde_json::from_str(&body).expect("応答を読めなかった");
    assert_eq!(updated["aiAllowed"], true, "{body}");
    assert_eq!(updated["username"], "taro", "{body}");

    let availability = get_json(app.clone(), AVAILABILITY_URI, &taro).await;
    assert_eq!(availability["available"], true, "{availability}");
    let (code, body) = generate(app.clone(), &uri, &taro).await;
    assert_eq!(code, StatusCode::OK, "{body}");

    // ユーザーを消しても、使った量は使った時点の名前で残る。
    let (code, body) = send_empty(
        app.clone(),
        "DELETE",
        &format!("/api/v1/admin/users/{taro_id}"),
        &admin,
    )
    .await;
    assert_eq!(code, StatusCode::NO_CONTENT, "{body}");
    let usage = get_json(app, USAGE_URI, &admin).await;
    assert_eq!(usage.as_array().map(Vec::len), Some(1), "{usage}");
    assert!(usage[0]["userId"].is_null(), "{usage}");
    assert_eq!(usage[0]["username"], "taro", "{usage}");
    assert_eq!(usage[0]["used"], 1234, "{usage}");
}

#[sqlx::test]
async fn updating_ai_allowed_of_an_unknown_user_is_not_found(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    let (code, body) = send_json(
        app,
        "PUT",
        "/api/v1/admin/users/9999/ai-allowed",
        &cookie,
        r#"{"aiAllowed":true}"#,
    )
    .await;
    assert_error(code, &body, StatusCode::NOT_FOUND, "not_found");
}

/// 登録していない設置では、管理者でも使えない。
#[sqlx::test]
async fn ai_is_unavailable_until_the_pc_is_linked(pool: SqlitePool) {
    let AiApp { app, cookie, .. } = ai_app(&pool, false).await;
    let availability = get_json(app.clone(), AVAILABILITY_URI, &cookie).await;
    assert_eq!(availability["available"], false, "{availability}");

    let (uri, _dir) = generate_uri(app.clone(), &cookie, "ai-unlinked").await;
    let (code, body) = generate(app, &uri, &cookie).await;
    assert_error(code, &body, StatusCode::CONFLICT, "ai_not_linked");
}

/// トークンはこの PC と窓口のアカウントを結ぶものなので、バックアップに入れない。
#[sqlx::test]
async fn backup_does_not_contain_the_app_token(pool: SqlitePool) {
    use std::io::Read;

    let AiApp {
        app,
        cookie,
        account_path,
        ..
    } = ai_app(&pool, true).await;
    assert!(account_path.exists());

    let response = send_raw(
        app,
        build_request("GET", "/api/v1/admin/backup", Some(&cookie), None),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("バックアップを読めなかった");
    let mut zip =
        zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).expect("zip として読めなかった");
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).expect("zip の中身を読めなかった");
        let name = entry.name().to_string();
        assert!(
            name == "manifest.json" || name == "weblav.db" || name.starts_with("blobs/"),
            "バックアップに想定外のファイルがある: {name}"
        );
        let mut content = Vec::new();
        entry
            .read_to_end(&mut content)
            .expect("zip の中身を読めなかった");
        assert!(
            !content
                .windows(TOKEN.len())
                .any(|window| window == TOKEN.as_bytes()),
            "{name} にトークンが入っている"
        );
    }
}

/// 簡易チャットは、質問を窓口へ送って答えの文を返し、使った量を `chat` として残す。
#[sqlx::test]
async fn chat_relays_the_question_and_records_the_usage(pool: SqlitePool) {
    let AiApp {
        app, cookie, fake, ..
    } = ai_app(&pool, true).await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/ai/chat",
        &cookie,
        r#"{"prompt":"  富士山の高さは？ "}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let reply: serde_json::Value = serde_json::from_str(&body).expect("JSON でない");
    assert_eq!(reply["answer"], r#"{"axes":[]}"#, "{body}");

    {
        let fake = fake.lock().expect("鍵が毒された");
        assert_eq!(fake.prompts.len(), 1);
        assert_eq!(fake.prompts[0]["user"], "富士山の高さは？");
        assert!(fake.prompts[0]["system"].is_string());
        // 窓口が思考の強さを決められるよう、用途を伝える (ほかの機能は伝えない)。
        assert_eq!(fake.prompts[0]["task"], "chat");
    }
    let kinds: Vec<String> = usage_rows(&pool)
        .await
        .into_iter()
        .map(|(_, _, kind, _)| kind)
        .collect();
    assert_eq!(kinds, ["chat"]);

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/v1/ai/chat",
        &cookie,
        r#"{"prompt":"  "}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
}
