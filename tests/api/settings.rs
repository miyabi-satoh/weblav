//! サイトの設定・サーバーの設定・ログ。

use super::*;

const SITE_SETTINGS_URI: &str = "/api/v1/site-settings";
const ADMIN_SITE_SETTINGS_URI: &str = "/api/v1/admin/site-settings";

/// サイト設定は未ログインでも読め、既定は空文字列 (→ docs/ui.md「UI 全般」)。
#[sqlx::test]
async fn site_settings_are_empty_by_default_and_readable_anonymously(pool: SqlitePool) {
    let app = test_app(pool).await;

    let (status, body) = send_anon(app, "GET", SITE_SETTINGS_URI).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, r#"{"siteName":"","homeHeading":""}"#);
}

/// admin は保存でき、前後の空白は落とされる。空白だけの入力は未設定になる。
#[sqlx::test]
async fn admin_can_update_site_settings_with_trimmed_values(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;
    let expected = r#"{"siteName":"サンプル事務所","homeHeading":""}"#;

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        ADMIN_SITE_SETTINGS_URI,
        &cookie,
        r#"{"siteName":"  サンプル事務所 ","homeHeading":"   "}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, expected);

    let (status, body) = send_anon(app, "GET", SITE_SETTINGS_URI).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, expected);
}

/// 上限の文字数を超えると 422 になる。数え方 (バイト数でなく文字) と境目は `within_limit` の単体テストで見る。
#[sqlx::test]
async fn site_settings_update_limits_each_value_to_100_chars(pool: SqlitePool) {
    let (app, cookie) = admin_app(&pool).await;

    let (status, body) = send_json(
        app,
        "PUT",
        ADMIN_SITE_SETTINGS_URI,
        &cookie,
        &format!(r#"{{"siteName":"{}","homeHeading":""}}"#, "あ".repeat(101)),
    )
    .await;

    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
}

// --- サーバーの設定 (→ docs/architecture.md「スキーマ」) ---

const ADMIN_SERVER_SETTINGS_URI: &str = "/api/v1/admin/server-settings";

/// 設定とデータの置き場 (`config.toml`・ログ) を返すために、`test_app` の代わりに使う。
/// 置き場は戻り値を持っている間だけ残る。
async fn app_with_temp_dir(pool: SqlitePool, name: &str) -> (Router, test_support::TempDir) {
    let data_dir = temp_test_dir(name);
    let app = app_in(pool, &Config::default(), &data_dir).await;
    (app, data_dir)
}

/// 保存すると config.toml に書き、手で書いたほかの項目は残す。動いている値は変わらない (次の起動から効く)。
#[sqlx::test]
async fn admin_can_save_server_settings_to_config_file(pool: SqlitePool) {
    let (app, dir) = app_with_temp_dir(pool.clone(), "server-settings-save").await;
    let config_path = dir.join("config.toml");
    std::fs::write(&config_path, "# 手で書いた\n[log]\nfilter = \"debug\"\n")
        .expect("config.toml を作れなかった");
    let cookie = admin_cookie(app.clone(), &pool).await;
    let defaults = r#"{"port":3000,"uploadMaxSizeMb":100,"sessionExpiryDays":14}"#;

    let (status, body) = send_empty(app.clone(), "GET", ADMIN_SERVER_SETTINGS_URI, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        format!(r#"{{"saved":{defaults},"running":{defaults}}}"#)
    );

    let saved = r#"{"port":3100,"uploadMaxSizeMb":500,"sessionExpiryDays":30}"#;
    let (status, body) = send_json(app, "PUT", ADMIN_SERVER_SETTINGS_URI, &cookie, saved).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, format!(r#"{{"saved":{saved},"running":{defaults}}}"#));

    let text = std::fs::read_to_string(&config_path).expect("config.toml を読めなかった");
    assert!(text.contains("# 手で書いた"), "{text}");
    assert!(text.contains("filter = \"debug\""), "{text}");
    let config: Config = toml::from_str(&text).expect("書いた config.toml が読めない");
    assert_eq!(config.server.port, 3100);
    assert_eq!(config.upload.max_size_mb, 500);
    assert_eq!(config.session.expiry_days, 30);
}

/// config.toml がシンボリックリンクなら、リンクを残してリンク先を書き換える。手で絞った権限も戻さない。
#[cfg(unix)]
#[sqlx::test]
async fn server_settings_keep_symlink_and_permissions_of_config_file(pool: SqlitePool) {
    use std::os::unix::fs::PermissionsExt;

    let (app, dir) = app_with_temp_dir(pool.clone(), "server-settings-symlink").await;
    let real_path = dir.join("real.toml");
    std::fs::write(&real_path, "").expect("リンク先を作れなかった");
    std::fs::set_permissions(&real_path, std::fs::Permissions::from_mode(0o600))
        .expect("権限を変えられなかった");
    let config_path = dir.join("config.toml");
    std::os::unix::fs::symlink(&real_path, &config_path).expect("リンクを作れなかった");
    let cookie = admin_cookie(app.clone(), &pool).await;

    let (status, body) = send_json(
        app,
        "PUT",
        ADMIN_SERVER_SETTINGS_URI,
        &cookie,
        r#"{"port":3100,"uploadMaxSizeMb":100,"sessionExpiryDays":14}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        std::fs::symlink_metadata(&config_path)
            .expect("config.toml の情報を読めなかった")
            .is_symlink()
    );
    let config: Config =
        toml::from_str(&std::fs::read_to_string(&real_path).expect("リンク先を読めなかった"))
            .expect("書いた config.toml が読めない");
    assert_eq!(config.server.port, 3100);
    let mode = std::fs::metadata(&real_path)
        .expect("リンク先の情報を読めなかった")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

/// リンク先がまだ無いシンボリックリンクでも、リンクを残してリンク先に作る。
#[cfg(unix)]
#[sqlx::test]
async fn server_settings_create_the_target_of_a_dangling_config_symlink(pool: SqlitePool) {
    let (app, dir) = app_with_temp_dir(pool.clone(), "server-settings-dangling").await;
    let config_path = dir.join("config.toml");
    std::os::unix::fs::symlink("real.toml", &config_path).expect("リンクを作れなかった");
    let cookie = admin_cookie(app.clone(), &pool).await;

    let (status, body) = send_json(
        app,
        "PUT",
        ADMIN_SERVER_SETTINGS_URI,
        &cookie,
        r#"{"port":3100,"uploadMaxSizeMb":100,"sessionExpiryDays":14}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        std::fs::symlink_metadata(&config_path)
            .expect("config.toml の情報を読めなかった")
            .is_symlink()
    );
    let text = std::fs::read_to_string(dir.join("real.toml")).expect("リンク先を読めなかった");
    assert_eq!(
        toml::from_str::<Config>(&text)
            .expect("書いた config.toml が読めない")
            .server
            .port,
        3100
    );
}

/// 範囲の外の値は 422。config.toml は書き換えない。
#[sqlx::test]
async fn server_settings_reject_out_of_range_values(pool: SqlitePool) {
    let (app, dir) = app_with_temp_dir(pool.clone(), "server-settings-range").await;
    let config_path = dir.join("config.toml");
    let cookie = admin_cookie(app.clone(), &pool).await;

    let (status, body) = send_json(
        app,
        "PUT",
        ADMIN_SERVER_SETTINGS_URI,
        &cookie,
        r#"{"port":3000,"uploadMaxSizeMb":100,"sessionExpiryDays":0}"#,
    )
    .await;
    assert_error(
        status,
        &body,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request_body",
    );
    assert!(!config_path.exists());
}

/// 読めない config.toml は上書きせず 409 で知らせる。手で書いた内容を消さないため。
#[sqlx::test]
async fn server_settings_do_not_overwrite_a_broken_config_file(pool: SqlitePool) {
    let (app, dir) = app_with_temp_dir(pool.clone(), "server-settings-broken").await;
    let config_path = dir.join("config.toml");
    std::fs::write(&config_path, "[server\n").expect("config.toml を作れなかった");
    let cookie = admin_cookie(app.clone(), &pool).await;

    let (status, body) = send_empty(app.clone(), "GET", ADMIN_SERVER_SETTINGS_URI, &cookie).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    let (status, body) = send_json(
        app,
        "PUT",
        ADMIN_SERVER_SETTINGS_URI,
        &cookie,
        r#"{"port":3100,"uploadMaxSizeMb":100,"sessionExpiryDays":14}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        std::fs::read_to_string(&config_path).expect("config.toml を読めなかった"),
        "[server\n"
    );
}

// --- ログ (→ docs/ui.md「UI 全般」) ---

const ADMIN_LOG_SETTINGS_URI: &str = "/api/v1/admin/log-settings";
const ADMIN_LOGS_URI: &str = "/api/v1/admin/logs";

/// 詳しいログの切り替えは、次に読んだときにも残る (起動し直すまで)。
#[sqlx::test]
async fn admin_can_switch_verbose_logs(pool: SqlitePool) {
    let (app, _dir) = app_with_temp_dir(pool.clone(), "log-settings-switch").await;
    let cookie = admin_cookie(app.clone(), &pool).await;

    let (status, body) = send_empty(app.clone(), "GET", ADMIN_LOG_SETTINGS_URI, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, r#"{"verbose":false,"fileOutput":true}"#);

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        ADMIN_LOG_SETTINGS_URI,
        &cookie,
        r#"{"verbose":true}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, r#"{"verbose":true,"fileOutput":true}"#);

    let (status, body) = send_empty(app, "GET", ADMIN_LOG_SETTINGS_URI, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, r#"{"verbose":true,"fileOutput":true}"#);
}

/// ログのファイルだけを、古い日から順に見出しを付けてつなげ、添付として返す。
#[sqlx::test]
async fn admin_can_download_logs_oldest_first(pool: SqlitePool) {
    let (app, dir) = app_with_temp_dir(pool.clone(), "logs-download").await;
    let log_dir = AppDirs {
        config_dir: dir.to_path_buf(),
        data_dir: dir.to_path_buf(),
    }
    .log_dir();
    std::fs::create_dir_all(&log_dir).expect("ログの置き場を作れなかった");
    // 改行で終わらない (書きかけの) ファイルも、次の見出しは行の頭から始まる。
    std::fs::write(log_dir.join("weblav.log.2026-09-25"), "newer").expect("ログを作れなかった");
    std::fs::write(log_dir.join("weblav.log.2026-09-24"), "older\n").expect("ログを作れなかった");
    std::fs::write(log_dir.join("other.txt"), "not a log").expect("ファイルを作れなかった");
    let cookie = admin_cookie(app.clone(), &pool).await;

    let response = send_raw(
        app,
        build_request("GET", ADMIN_LOGS_URI, Some(&cookie), None),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/plain; charset=utf-8"
    );
    assert_eq!(
        response.headers()[header::CONTENT_DISPOSITION],
        r#"attachment; filename="weblav-logs.txt""#
    );
    assert_eq!(
        body_string(response).await,
        "===== weblav.log.2026-09-24 =====\nolder\n\n===== weblav.log.2026-09-25 =====\nnewer"
    );
}

/// まだログが1つも無ければ、空のテキストを返す。
#[sqlx::test]
async fn downloading_logs_before_any_are_written_returns_empty_text(pool: SqlitePool) {
    let (app, _dir) = app_with_temp_dir(pool.clone(), "logs-download-empty").await;
    let cookie = admin_cookie(app.clone(), &pool).await;

    let (status, body) = send_empty(app, "GET", ADMIN_LOGS_URI, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, "");
}

/// ログを標準出力に出しているときは、ダウンロードするものが無い。
#[sqlx::test]
async fn logs_cannot_be_downloaded_when_written_to_stdout(pool: SqlitePool) {
    let dir = temp_test_dir("logs-stdout");
    let mut config = Config::default();
    config.log.output = LogOutput::Stdout;
    let app = app_in(pool.clone(), &config, &dir).await;
    let cookie = admin_cookie(app.clone(), &pool).await;

    let (status, body) = send_empty(app.clone(), "GET", ADMIN_LOG_SETTINGS_URI, &cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, r#"{"verbose":false,"fileOutput":false}"#);

    let (status, body) = send_empty(app, "GET", ADMIN_LOGS_URI, &cookie).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}
