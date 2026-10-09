pub mod api;
pub mod auth;
pub mod cli;
pub mod config;
pub mod db;
pub mod error;
pub mod file_ext;
pub mod folder_access;
pub mod folder_picker;
pub mod logging;
pub mod mdns;
pub mod os_thumbnail;
pub mod pro;
pub mod server;
pub mod session;
pub mod single_instance;
pub mod state;
pub mod static_files;
#[cfg(test)]
#[path = "test_support.rs"]
mod test_support;

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::http::{HeaderValue, header};
use sqlx::SqlitePool;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tower_sessions::cookie::Key;
use tower_sessions_sqlx_store::SqliteStore;

use config::{AppDirs, Config};
use state::AppState;

/// アプリの版 (`Cargo.toml` の `version`)。
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// ビルド番号 (ビルドしたコミットまでのコミットの数。`build.rs` が数える)。
/// git の無い場所でビルドしたときは無い。
pub const APP_BUILD: Option<&str> = option_env!("WEBLAV_BUILD");

/// アプリケーション全体の `Router` を組み立てる。セッションストア用テーブルの作成もここで
/// 行う。`main.rs` と統合テスト(`tests/api/`)の両方から共通で呼べるように公開している。
///
/// `dirs` から、ファイルアップロード型コンテンツの実体 (`blobs_dir`)・画像の縮小画像
/// (`thumbnails_dir`)・管理画面から書き換える設定ファイル (`config_path`) の置き場を取る。
/// どれもディレクトリの作成は最初に書くときまで遅らせる (DB・セッション鍵と同じ方針)。
/// 設定とデータの置き場そのものは、公開できるフォルダーの中でも見せない場所として判定に使う
/// (→ `api::roots::OwnDirs`)。
///
/// `config` は動いている値として管理画面に出す (→ `api::server_settings`)。
///
/// `folder_picker` は「公開できるフォルダー」を選ぶ窓を出す手段 (→ `folder_picker`)。
pub async fn build_app(
    pool: SqlitePool,
    session_key: Key,
    config: &Config,
    dirs: &AppDirs,
    lan_port: Option<u16>,
    folder_picker: folder_picker::FolderPicker,
) -> Result<Router, Box<dyn std::error::Error + Send + Sync>> {
    let session_store = SqliteStore::new(pool.clone());
    session_store.migrate().await?;

    // secure_cookie無効のまま非loopbackで待ち受けると、非HTTPS環境ではCookieが
    // ネットワーク上を平文で流れる。secretの有無はこのリスクを変えない(永続化された
    // ランダム64バイト鍵で足りる、session.rs参照)ため、起動は止めず記録だけ残す。
    //
    // 既定の構成 (bind = 0.0.0.0・HTTPS終端なし) でも必ず通るので warn にはしない。
    // 推奨どおりに設置した管理者が毎回警告を見ることになり、ログ全体を無視させる。
    // 前段にHTTPS終端を置いた運用者だけが true にするものなので、その案内に留める。
    if !config.session.secure_cookie && !config.server.bind.is_loopback() {
        tracing::info!(
            bind = %config.server.bind,
            "serving plain HTTP on a non-loopback address; session cookies are not marked Secure. \
             Set session.secure_cookie = true only if HTTPS is terminated in front of the server"
        );
    }

    // 前に動いていたときに、バックアップの作成や戻しの途中で止まった残りを消す。
    let backup_work_dir = dirs.backup_work_dir();
    {
        let dir = backup_work_dir.clone();
        tokio::task::spawn_blocking(move || api::backup::remove_work_root(&dir)).await?;
    }

    let pro = {
        let path = dirs.pro_path();
        Arc::new(tokio::task::spawn_blocking(move || pro::Pro::load(&path)).await?)
    };

    let revoked_sessions = Arc::new(session::RevokedSessions::default());
    let session_layer = session::layer(
        session::Store::new(session_store, Arc::clone(&revoked_sessions)),
        session_key,
        &config.session,
    );

    // ログイン試行のレートリミット: 同一ユーザー名につき1分間に5回まで。
    // 異なるユーザー名を使った回避・CPU枯渇DoSを防ぐため、全体でも1分間に30回までの
    // グローバル上限を課す。
    let login_rate_limiter = Arc::new(auth::LoginRateLimiter::new(5, 30, Duration::from_secs(60)));
    // リカバリコードでの再設定も同じ回数で絞る。枠はログインと分ける (→ docs/access.md「リカバリコード」)。
    let recovery_rate_limiter =
        Arc::new(auth::LoginRateLimiter::new(5, 30, Duration::from_secs(60)));

    let max_upload_bytes = config.upload.max_bytes();
    let (router, _openapi) = api::routed(max_upload_bytes).split_for_parts();
    let app = router
        .fallback(static_files::handler)
        .layer(TraceLayer::new_for_http())
        // `ServeFile`で返す音声・動画・PDF・任意バイナリ(contents.rs)は圧縮しない。
        // Range/Content-Lengthが失われ、圧縮済み形式を再圧縮する分CPUも無駄になるため。
        // これらは既定の除外(gRPC/image/SSE/32B未満)に含まれないので、既定の除外を
        // 残したまま`DefaultPredicate`に条件を重ねる。
        .layer(
            CompressionLayer::new().compress_when(
                DefaultPredicate::new()
                    .and(NotForContentType::const_new("audio/"))
                    .and(NotForContentType::const_new("video/"))
                    .and(NotForContentType::const_new("application/pdf"))
                    .and(NotForContentType::const_new("application/octet-stream")),
            ),
        )
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        // `route_layer`は登録済みルートにしか掛からず`fallback`には掛からない
        // (axum 0.8 route_layer.md)。静的ファイル配信への不要なDBアクセスを避けるため
        // `layer`ではなくこちらを使う。
        .route_layer(session_layer);
    let state = AppState {
        pool,
        login_rate_limiter,
        recovery_rate_limiter,
        blobs_dir: dirs.blobs_dir(),
        thumbnails: Arc::new(api::thumbnails::Thumbnails::new(dirs.thumbnails_dir())),
        link_previews: Arc::new(api::link_preview::LinkPreviews::new(
            dirs.link_previews_dir(),
        )),
        own_dirs: vec![dirs.config_dir.clone(), dirs.data_dir.clone()],
        config_path: dirs.config_path(),
        running_server_settings: api::server_settings::ServerSettings::from_config(config),
        log: logging::LogControl::current_or_detached(config, dirs),
        config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
        contents_write_lock: Arc::new(tokio::sync::Mutex::new(())),
        archive_scans: Arc::new(crate::state::ArchiveScans::default()),
        users_write_lock: Arc::new(tokio::sync::Mutex::new(())),
        setup_tokens: Arc::new(api::setup::SetupTokens::default()),
        backup_work_dir,
        restore_staging: Arc::new(api::backup::RestoreStaging::default()),
        revoked_sessions,
        max_upload_bytes,
        lan_port,
        folder_picker,
        pro,
        pro_renewal: Arc::new(api::pro::Renewal::new(
            api::pro::ACCOUNT_SERVER_URL.to_string(),
        )),
    };
    // 使われたときに Pro を確かめる (→ docs/pro.md「確かめる」)。静的ファイルや閲覧も「使われた」に数えるので、全体に掛ける。
    let app = app
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            api::pro::renew_when_due,
        ))
        .with_state(state);

    Ok(app)
}
