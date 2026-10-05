//! 管理画面の Pro の区画: 今の状態を見せ、窓口のアカウントに結ぶ・確かめる・外す
//! (→ docs/pro.md「結ぶ・確かめる・外す (WebLAV の側)」)。`admin` だけが使える。
//! 使われたときの裏の確かめ (`renew_when_due`) も、窓口とのやり取りなのでここに置く。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use ureq::Agent;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AdminUser;
use crate::error::{AppError, AppJson, error_chain, run_blocking};
use crate::pro::{CheckOutcome, CheckRequest, Edition, PermissionKind, Plan, Pro, Refusal};
use crate::state::AppState;

use super::free_limit::{self, FreeLimitUsage};

/// 配る版がつなぐ窓口 (→ docs/pro.md「アカウントと販売の窓口」)。
#[cfg(not(debug_assertions))]
pub const ACCOUNT_SERVER_URL: &str = "https://weblav.amiiby.com";

/// 開発版がつなぐ、手元で動かす窓口 (→ `just dev-account-server`)。
/// 開発版は本番の鍵を信じないので、本番の窓口につないでも Pro にならず、本番に記録だけが残る。
#[cfg(debug_assertions)]
pub const ACCOUNT_SERVER_URL: &str = "http://127.0.0.1:8787";

/// 窓口への1回の問い合わせの時間切れ。窓口が応えないときに、画面の操作を長く待たせないように。
const TIMEOUT: Duration = Duration::from_secs(10);

/// 結びかけを画面から問い合わせるとき、窓口に確かめる最も短い間隔。画面は3秒おきに呼ぶ。
const PENDING_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// 裏の確かめに失敗したら、この間は試し直さない (→ docs/pro.md「確かめる」)。窓口が止まっている間、要求のたびにつながりに行かないように。
const RETRY_AFTER: Duration = Duration::from_secs(3600);

/// 窓口とのやり取りの状態。裏の確かめが重ならないように、また失敗の後に間を空けるために持つ。
#[derive(Debug)]
pub struct Renewal {
    server: String,
    in_flight: AtomicBool,
    last_failure: std::sync::Mutex<Option<Instant>>,
    last_pending_poll: tokio::sync::Mutex<Option<Instant>>,
}

impl Renewal {
    pub fn new(server: String) -> Self {
        Self {
            server,
            in_flight: AtomicBool::new(false),
            last_failure: std::sync::Mutex::new(None),
            last_pending_poll: tokio::sync::Mutex::new(None),
        }
    }
}

/// 使われたときに、確かめに行く頃なら裏で確かめる (→ docs/pro.md「確かめる」)。応答は待たせない。
pub async fn renew_when_due(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let pro = state.pro.clone();
    let renewal = state.pro_renewal.clone();
    let recently_failed = renewal
        .last_failure
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .is_some_and(|at| at.elapsed() < RETRY_AFTER);
    if !recently_failed
        && let Some(check) = pro.due_check()
        && !renewal.in_flight.swap(true, Ordering::AcqRel)
    {
        let pool = state.pool.clone();
        let pro = pro.clone();
        tokio::spawn(async move {
            let name = site_name(&pool).await;
            let result = run_check(&pro, &renewal.server, check, name).await;
            let failed = result.is_err();
            if let Err(err) = result {
                tracing::warn!(err = %error_chain(&err), "failed to check Pro with the account server");
            }
            *renewal
                .last_failure
                .lock()
                .unwrap_or_else(|err| err.into_inner()) = failed.then(Instant::now);
            renewal.in_flight.store(false, Ordering::Release);
        });
    }
    if pro.high_water_due() {
        tokio::task::spawn_blocking(move || pro.persist_high_water());
    }
    next.run(request).await
}

/// 窓口に確かめ、答えで手元を合わせる。結びかけで、まだ結ばれていなければ `Ok(false)`。
async fn run_check(
    pro: &Arc<Pro>,
    server: &str,
    check: CheckRequest,
    name: String,
) -> Result<bool, AppError> {
    let server = server.to_string();
    let target = check.target.clone();
    let outcome = run_blocking(move || post_check(&server, &check, &name)).await??;
    let pro = pro.clone();
    run_blocking(move || pro.apply_check(&target, outcome))
        .await?
        .map_err(AppError::from)
}

/// 窓口に出す名前 (サイト名)。読めなければ空にし、窓口は前の名前のままにする。
async fn site_name(pool: &sqlx::SqlitePool) -> String {
    super::site_settings::load(pool)
        .await
        .map(|s| s.site_name)
        .unwrap_or_default()
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProResponse {
    edition: Edition,
    /// Pro のときだけ。
    plan: Option<Plan>,
    /// 上限のある対象ごとの今の件数と Free の上限。Pro でも返す (上限は効いていない)。
    usage: Vec<FreeLimitUsage>,
    /// アカウントに結んでいるか。
    bound: bool,
    /// 結んだアカウントのメールアドレス。ネットで確かめるまで分からない (返しのコードで結んだとき)。
    email: Option<String>,
    /// 今の許可の期限 (UNIX 秒)。
    expires_at: Option<i64>,
    /// 許可をネットで受け取ったか、返しのコードで受け取ったか。
    permission: Option<PermissionKind>,
    last_checked_at: Option<i64>,
    /// 窓口が直近に許可を出さなかったわけ。
    refusal: Option<Refusal>,
    /// PC の時計が前に見た時刻より遅れていて、Free として動いている。
    clock_behind: bool,
    /// 結びかけなら、窓口の結ぶ画面の URL (QR コードとボタンに使う)。
    link_url: Option<String>,
    /// 窓口のアカウントのページ。
    account_url: String,
}

async fn response(state: &AppState) -> Result<ProResponse, AppError> {
    // 1回だけ読む。項目ごとに読むと、間に変わったときに食い違う。
    let snapshot = state.pro.snapshot();
    let link_url = match snapshot.pending_request {
        Some(request) => Some(link_url(
            &state.pro_renewal.server,
            &request,
            &site_name(&state.pool).await,
        )),
        None => None,
    };
    Ok(ProResponse {
        edition: Edition::of(snapshot.plan),
        plan: snapshot.plan,
        usage: free_limit::usage(&state.pool).await?,
        bound: snapshot.bound,
        email: snapshot.email,
        expires_at: snapshot.expires_at,
        permission: snapshot.kind,
        last_checked_at: snapshot.last_checked_at,
        refusal: snapshot.refusal,
        clock_behind: snapshot.clock_behind,
        link_url,
        account_url: format!("{}/account/", state.pro_renewal.server),
    })
}

/// 窓口の結ぶ画面の URL。言語は画面が足す。
fn link_url(server: &str, request: &str, name: &str) -> String {
    with_query(
        &format!("{server}/account/link"),
        &[("r", request), ("name", name)],
    )
}

#[utoipa::path(
    get,
    path = "/admin/pro",
    responses(
        (status = OK, body = ProResponse, description = "Pro の状態と今の件数"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
    )
)]
async fn get_pro(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ProResponse>, AppError> {
    Ok(Json(response(&state).await?))
}

/// 結びかけを作る (→ docs/pro.md「結ぶ」)。前の結びかけは捨てる。結んであれば、同じ枠の結び直しにする。
#[utoipa::path(
    post,
    path = "/admin/pro/link",
    responses(
        (status = OK, body = ProResponse, description = "結びかけた後の状態 (`linkUrl` が入る)"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
    )
)]
async fn start_link(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ProResponse>, AppError> {
    let pro = state.pro.clone();
    run_blocking(move || pro.start_link()).await??;
    Ok(Json(response(&state).await?))
}

/// 結びかけを捨てる (「やめる」)。
#[utoipa::path(
    delete,
    path = "/admin/pro/link",
    responses(
        (status = OK, body = ProResponse, description = "捨てた後の状態"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
    )
)]
async fn cancel_link(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ProResponse>, AppError> {
    let pro = state.pro.clone();
    run_blocking(move || pro.cancel_link()).await??;
    Ok(Json(response(&state).await?))
}

/// 結びかけが窓口で結ばれたかを確かめる (ネットにつながる WebLAV)。結ばれていれば証明を受け取って書く。
/// 窓口には `PENDING_POLL_INTERVAL` より短くは確かめない (間に来た分は今の状態だけを返す)。
#[utoipa::path(
    post,
    path = "/admin/pro/link/poll",
    responses(
        (status = OK, body = ProResponse, description = "今の状態。結ばれていれば `linkUrl` が消える"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 409, body = crate::error::ErrorResponse, description = "PC の時計が証明の発行日より遅れている"),
        (status = 502, body = crate::error::ErrorResponse, description = "窓口につながらない・証明を確かめられない"),
    )
)]
async fn poll_link(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ProResponse>, AppError> {
    let mut last = state.pro_renewal.last_pending_poll.lock().await;
    if let Some(check) = state.pro.pending_check()
        && last.is_none_or(|at| at.elapsed() >= PENDING_POLL_INTERVAL)
    {
        *last = Some(Instant::now());
        let name = site_name(&state.pool).await;
        run_check(&state.pro, &state.pro_renewal.server, check, name).await?;
    }
    drop(last);
    Ok(Json(response(&state).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CodeRequest {
    /// 窓口の画面かメールに出た返しのコード (15文字。区切り・大小文字は問わない)。
    code: String,
}

/// 返しのコードを受け取る (ネットにつながらない WebLAV)。
#[utoipa::path(
    post,
    path = "/admin/pro/link/code",
    request_body = CodeRequest,
    responses(
        (status = OK, body = ProResponse, description = "結んだ後の状態"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 409, body = crate::error::ErrorResponse, description = "結びかけていない・PC の時計が遅れている"),
        (status = 422, body = crate::error::ErrorResponse, description = "コードが合わない"),
    )
)]
async fn accept_code(
    _admin: AdminUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<CodeRequest>,
) -> Result<Json<ProResponse>, AppError> {
    let pro = state.pro.clone();
    run_blocking(move || pro.accept_code(&payload.code)).await??;
    Ok(Json(response(&state).await?))
}

/// 条件によらず、その場で窓口に確かめる (「今すぐ確かめる」)。買った直後・外した直後に合わせるため。
#[utoipa::path(
    post,
    path = "/admin/pro/check",
    responses(
        (status = OK, body = ProResponse, description = "確かめた後の状態"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 409, body = crate::error::ErrorResponse, description = "結んでいない・PC の時計が証明の発行日より遅れている"),
        (status = 502, body = crate::error::ErrorResponse, description = "窓口につながらない・証明を確かめられない"),
    )
)]
async fn check_now(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ProResponse>, AppError> {
    let check = state
        .pro
        .bound_check()
        .ok_or_else(|| AppError::Conflict("not linked".to_string()))?;
    let name = site_name(&state.pool).await;
    run_check(&state.pro, &state.pro_renewal.server, check, name).await?;
    Ok(Json(response(&state).await?))
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnlinkResponse {
    pro: ProResponse,
    /// 窓口へ外したことを届けられなかったときだけ。スマートフォンで読んでもらう外した証しの URL。
    release_url: Option<String>,
    /// 同じく、URL を開けないときに打ち込む外した証し。
    release_code: Option<String>,
}

/// この WebLAV をアカウントから外す (→ docs/pro.md「外す」)。Free に戻し、外した証しを窓口へ送る。
#[utoipa::path(
    delete,
    path = "/admin/pro",
    responses(
        (status = OK, body = UnlinkResponse, description = "外した後の状態"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
    )
)]
async fn unlink(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<UnlinkResponse>, AppError> {
    let pro = state.pro.clone();
    let release = run_blocking(move || pro.unbind()).await??;
    let mut undelivered = None;
    if let Some(code) = release {
        let server = state.pro_renewal.server.clone();
        let sent = code.clone();
        // 届かなくても手元は外れている。窓口の枠が空かないだけなので、証しを画面に出して読んでもらう。
        if let Err(err) = run_blocking(move || post_release(&server, &sent)).await? {
            tracing::warn!(err = %error_chain(&err), "failed to tell the account server about the removal");
            undelivered = Some(code);
        }
    }
    Ok(Json(UnlinkResponse {
        pro: response(&state).await?,
        release_url: undelivered.as_ref().map(|code| {
            with_query(
                &format!("{}/account/release", state.pro_renewal.server),
                &[("c", code)],
            )
        }),
        release_code: undelivered,
    }))
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_pro, unlink))
        .routes(routes!(start_link, cancel_link))
        .routes(routes!(poll_link))
        .routes(routes!(accept_code))
        .routes(routes!(check_now))
}

// ---- 窓口とのやり取り ----

/// `base` にクエリを足す。値は URL の中で使える形に直す。
fn with_query(base: &str, pairs: &[(&str, &str)]) -> String {
    let query: Vec<String> = pairs
        .iter()
        .map(|(k, v)| {
            format!(
                "{k}={}",
                percent_encoding::utf8_percent_encode(v, percent_encoding::NON_ALPHANUMERIC)
            )
        })
        .collect();
    format!("{base}?{}", query.join("&"))
}

#[derive(Debug, Serialize)]
struct CheckBody<'a> {
    installation: &'a str,
    auth: &'a str,
    name: &'a str,
}

#[derive(Debug, Serialize)]
struct ReleaseBody<'a> {
    code: &'a str,
}

fn agent() -> Agent {
    Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .user_agent(concat!("WebLAV/", env!("CARGO_PKG_VERSION")))
        .tls_config(super::native_tls_config())
        .build()
        .into()
}

fn post_check(server: &str, check: &CheckRequest, name: &str) -> Result<CheckOutcome, AppError> {
    let body = serde_json::to_string(&CheckBody {
        installation: &check.installation,
        auth: &check.auth,
        name,
    })
    .map_err(|err| AppError::AccountServer(err.to_string()))?;
    let text = post(&format!("{server}/v1/installations/check"), &body)?;
    serde_json::from_str(&text).map_err(|err| AppError::AccountServer(format!("{server}: {err}")))
}

fn post_release(server: &str, code: &str) -> Result<(), AppError> {
    let body = serde_json::to_string(&ReleaseBody { code })
        .map_err(|err| AppError::AccountServer(err.to_string()))?;
    post(&format!("{server}/v1/installations/release"), &body).map(|_| ())
}

fn post(url: &str, body: &str) -> Result<String, AppError> {
    let unreachable =
        |err: &dyn std::fmt::Display| AppError::AccountServer(format!("{url}: {err}"));
    let mut response = agent()
        .post(url)
        .header("content-type", "application/json")
        .send(body)
        .map_err(|err| unreachable(&err))?;
    let status = response.status();
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|err| unreachable(&err))?;
    if !status.is_success() {
        return Err(unreachable(&format!("status {status}: {text}")));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 窓口の代わりに、決まった応答を返す HTTP サーバーを立てる。
    async fn fake_server(router: axum::Router) -> String {
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
        format!("http://{addr}")
    }

    fn check() -> CheckRequest {
        CheckRequest {
            target: crate::pro::CheckTarget::Bound("0000000000000000".to_string()),
            installation: "0000000000000000".to_string(),
            auth: "ab".to_string(),
        }
    }

    #[tokio::test]
    async fn check_sends_the_installation_and_reads_the_outcome() {
        let server = fake_server(axum::Router::new().route(
            "/v1/installations/check",
            axum::routing::post(|Json(body): Json<serde_json::Value>| async move {
                assert_eq!(body["installation"], "0000000000000000");
                assert_eq!(body["auth"], "ab");
                assert_eq!(body["name"], "教室");
                Json(serde_json::json!({ "status": "no_plan", "email": "a@example.com" }))
            }),
        ))
        .await;
        let outcome = run_blocking(move || post_check(&server, &check(), "教室"))
            .await
            .expect("確かめを走らせられなかった")
            .expect("確かめが失敗した");
        assert!(matches!(outcome, CheckOutcome::NoPlan { email } if email == "a@example.com"));
    }

    #[tokio::test]
    async fn error_status_is_an_account_server_error() {
        let server = fake_server(axum::Router::new().route(
            "/v1/installations/check",
            axum::routing::post(|| async { axum::http::StatusCode::INTERNAL_SERVER_ERROR }),
        ))
        .await;
        let result = run_blocking(move || post_check(&server, &check(), ""))
            .await
            .expect("確かめを走らせられなかった");
        assert!(matches!(result, Err(AppError::AccountServer(_))));
    }
}
