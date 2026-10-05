//! 管理画面から、詳しいログへの切り替えとログのダウンロードをする (→ docs/ui.md「UI 全般」)。
//! `admin` だけが使える。画面の中でログを読む機能は持たない。

use std::io::Cursor;
use std::path::PathBuf;
use std::pin::Pin;

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::io::ReaderStream;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AdminUser;
use crate::error::{AppError, AppJson, run_blocking};
use crate::logging;
use crate::state::AppState;

/// ダウンロードさせるときのファイル名。
const DOWNLOAD_FILE_NAME: &str = "weblav-logs.txt";

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogSettingsResponse {
    /// 詳しいログを出しているか。起動し直すと `false` に戻る。
    verbose: bool,
    /// ログをファイルに書いているか (`config.toml` の `log.output = "file"`)。書いていなければダウンロードできない。
    file_output: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateLogSettingsRequest {
    verbose: bool,
}

fn response(state: &AppState) -> LogSettingsResponse {
    LogSettingsResponse {
        verbose: state.log.verbose(),
        file_output: state.log.log_dir().is_some(),
    }
}

#[utoipa::path(
    get,
    path = "/admin/log-settings",
    responses(
        (status = OK, body = LogSettingsResponse, description = "ログの設定"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
    )
)]
async fn get_log_settings(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Json<LogSettingsResponse> {
    Json(response(&state))
}

/// 詳しいログを出す・やめる。起動し直さずにすぐ効き、`config.toml` には書かない。
#[utoipa::path(
    put,
    path = "/admin/log-settings",
    request_body = UpdateLogSettingsRequest,
    responses(
        (status = OK, body = LogSettingsResponse, description = "切り替えた後の設定"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
    )
)]
async fn update_log_settings(
    _admin: AdminUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<UpdateLogSettingsRequest>,
) -> Result<Json<LogSettingsResponse>, AppError> {
    state
        .log
        .set_verbose(payload.verbose)
        .map_err(|err| AppError::Io(std::io::Error::other(err)))?;
    tracing::info!(verbose = payload.verbose, "log verbosity changed");
    Ok(Json(response(&state)))
}

/// 残っているログ (最大14日分) を、古い日から順に1つのテキストにつなげて返す。
/// 渡す相手が1つ開けば済むよう、日ごとには分けない。
#[utoipa::path(
    get,
    path = "/admin/logs",
    responses(
        (status = OK, content_type = "text/plain", description = "ログをつなげたテキスト"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 404, body = crate::error::ErrorResponse, description = "ログをファイルに書いていない"),
    )
)]
async fn download_logs(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let dir = state.log.log_dir().ok_or(AppError::NotFound)?.to_path_buf();
    let reader = open_logs(dir).await?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                "text/plain; charset=utf-8".to_string(),
            ),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{DOWNLOAD_FILE_NAME}\""),
            ),
        ],
        Body::from_stream(ReaderStream::new(reader)),
    ))
}

/// ファイルごとに名前の見出しを付けて、古い日から順につなげる。まだ1つも無ければ空。
/// 詳しいログは14日分で大きくなりうるので、メモリに載せずに流す。
async fn open_logs(dir: PathBuf) -> Result<Pin<Box<dyn AsyncRead + Send>>, AppError> {
    let files = match run_blocking(move || logging::log_files(&dir)).await? {
        Ok(files) => files,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => return Err(AppError::Io(err)),
    };
    let mut reader: Pin<Box<dyn AsyncRead + Send>> = Box::pin(tokio::io::empty());
    let mut first = true;
    for path in files {
        let file = match tokio::fs::File::open(&path).await {
            Ok(file) => file,
            // 一覧を取った後に、日付が替わって古い日が消えた。
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(AppError::Io(err)),
        };
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        // 前のファイルが改行で終わっていなくても、見出しが行の頭に来るよう空行を挟む。
        let gap = if first { "" } else { "\n" };
        first = false;
        let heading = Cursor::new(format!("{gap}===== {name} =====\n").into_bytes());
        reader = Box::pin(reader.chain(heading).chain(file));
    }
    Ok(reader)
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_log_settings, update_log_settings))
        .routes(routes!(download_logs))
}
