//! 管理画面から変える、サーバーの設定 (→ docs/architecture.md「スキーマ」)。`admin` だけが読み書きできる。
//!
//! 値は `config.toml` に書き、次の起動から効く。ポートは動いている間に変えられず、
//! ほかの項目も扱いを揃えるため。
//! 書き換えは `toml_edit` で行い、手で書いたほかの項目やコメントは残す。

use std::path::Path;

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AdminUser;
use crate::config::{self, Config};
use crate::error::{AppError, AppJson, run_blocking};
use crate::state::AppState;

/// アップロードの上限の上限 (MiB)。100 GiB。桁を打ち間違えたときに気づけるよう、実用の範囲で止める。
const MAX_UPLOAD_MAX_SIZE_MB: u64 = 100 * 1024;
/// セッションの有効日数の上限。10年。
const MAX_SESSION_EXPIRY_DAYS: i64 = 3650;

/// 管理画面で変えられる設定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServerSettings {
    /// 待ち受けるポート。
    pub port: u16,
    /// ファイルのアップロード1件あたりの上限 (MiB)。
    pub upload_max_size_mb: u64,
    /// 最後に使ってから、ログインが切れるまでの日数。
    pub session_expiry_days: i64,
}

impl ServerSettings {
    pub fn from_config(config: &Config) -> Self {
        Self {
            port: config.server.port,
            upload_max_size_mb: config.upload.max_size_mb,
            session_expiry_days: config.session.expiry_days,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServerSettingsResponse {
    /// `config.toml` に書かれている値 (無ければ既定値)。次の起動から効く。
    saved: ServerSettings,
    /// 動いている値。ポートは実際に待ち受けている番号 (ずらして起動していれば、保存した値と違う)。
    running: ServerSettings,
}

#[utoipa::path(
    get,
    path = "/admin/server-settings",
    responses(
        (status = OK, body = ServerSettingsResponse, description = "保存した値と動いている値"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 409, body = crate::error::ErrorResponse, description = "config.toml を読めない"),
    )
)]
async fn get_server_settings(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ServerSettingsResponse>, AppError> {
    let guard = state.config_write_lock.clone().lock_owned().await;
    let path = state.config_path.clone();
    let saved = run_blocking(move || {
        let _guard = guard;
        read_saved(&path)
    })
    .await??;
    Ok(Json(ServerSettingsResponse {
        saved,
        running: state.running_server_settings,
    }))
}

#[utoipa::path(
    put,
    path = "/admin/server-settings",
    request_body = ServerSettings,
    responses(
        (status = OK, body = ServerSettingsResponse, description = "保存した後の値"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 409, body = crate::error::ErrorResponse, description = "config.toml を読めない"),
        (status = 422, body = crate::error::ErrorResponse, description = "範囲の外の値"),
    )
)]
async fn update_server_settings(
    _admin: AdminUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<ServerSettings>,
) -> Result<Json<ServerSettingsResponse>, AppError> {
    validate(&payload)?;
    // ロックはファイル操作の中で外す。接続が切れてハンドラが捨てられても、書き終わるまで
    // 次の保存を待たせるため (`spawn_blocking` の処理は止まらずに続く)。
    let guard = state.config_write_lock.clone().lock_owned().await;
    let path = state.config_path.clone();
    let saved = run_blocking(move || {
        let _guard = guard;
        write_saved(&path, &payload)?;
        read_saved(&path)
    })
    .await??;
    tracing::info!(
        ?payload,
        "server settings saved; effective from the next start"
    );
    Ok(Json(ServerSettingsResponse {
        saved,
        running: state.running_server_settings,
    }))
}

fn validate(settings: &ServerSettings) -> Result<(), AppError> {
    if settings.port == 0 {
        return Err(AppError::Validation("port must be 1 or greater".into()));
    }
    if !(1..=MAX_UPLOAD_MAX_SIZE_MB).contains(&settings.upload_max_size_mb) {
        return Err(AppError::Validation(format!(
            "uploadMaxSizeMb must be between 1 and {MAX_UPLOAD_MAX_SIZE_MB}"
        )));
    }
    if !(1..=MAX_SESSION_EXPIRY_DAYS).contains(&settings.session_expiry_days) {
        return Err(AppError::Validation(format!(
            "sessionExpiryDays must be between 1 and {MAX_SESSION_EXPIRY_DAYS}"
        )));
    }
    Ok(())
}

/// `config.toml` が壊れている (起動した後に手で書き換えた等) ときは、書き換えずに知らせる。
/// 読めない形のまま上書きすると、手で書いた内容を消してしまうため。
fn broken_config(err: impl std::fmt::Display) -> AppError {
    AppError::ConfigUnreadable(format!("config.toml cannot be read: {err}"))
}

fn read_saved(path: &Path) -> Result<ServerSettings, AppError> {
    let config = Config::load_from(path).map_err(|err| match err {
        config::Error::Read { source, .. } => AppError::Io(source),
        err => broken_config(crate::error::error_chain(&err)),
    })?;
    Ok(ServerSettings::from_config(&config))
}

fn write_saved(path: &Path, settings: &ServerSettings) -> Result<(), AppError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(AppError::Io(err)),
    };
    let updated = apply(&text, settings).map_err(broken_config)?;
    // 書いた結果を起動のときと同じ読み方で確かめる。読めない形を書くと、次に起動できなくなる。
    toml::from_str::<Config>(&updated).map_err(broken_config)?;

    if let Some(dir) = path.parent() {
        config::create_owner_only_dir(dir)?;
    }
    // シンボリックリンクなら、リンクを普通のファイルに置き換えず、リンク先を書き換える。
    let target = match std::fs::canonicalize(path) {
        Ok(target) => target,
        // リンク先がまだ無いリンクは辿れないので、リンクが指す先を読んでそこに作る。
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => match std::fs::read_link(path) {
            Ok(link) => path.parent().map_or(link.clone(), |dir| dir.join(&link)),
            Err(_) => path.to_path_buf(),
        },
        Err(err) => return Err(AppError::Io(err)),
    };
    // 途中で落ちても壊れた config.toml を残さないよう、隣に書いてから置き換える。
    // `session.secret` を書けるファイルなので、新しく作るときは所有者だけが読めるようにし、
    // 元があればその権限を引き継ぐ。
    config::replace_owner_only_file(
        &target.with_extension("toml.tmp"),
        &target,
        updated.as_bytes(),
        true,
    )
    .map_err(AppError::Io)
}

/// `text` の該当する項目だけを書き換える。表 (`[server]` など) が無ければ足す。
fn apply(text: &str, settings: &ServerSettings) -> Result<String, String> {
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|err| format!("{err}"))?;
    set(&mut doc, "server", "port", i64::from(settings.port))?;
    // 上限は `validate` で i64 に収まる範囲に絞ってある。
    set(
        &mut doc,
        "upload",
        "max_size_mb",
        settings.upload_max_size_mb as i64,
    )?;
    set(
        &mut doc,
        "session",
        "expiry_days",
        settings.session_expiry_days,
    )?;
    Ok(doc.to_string())
}

fn set(doc: &mut toml_edit::DocumentMut, table: &str, key: &str, value: i64) -> Result<(), String> {
    doc.entry(table)
        .or_insert(toml_edit::table())
        .as_table_like_mut()
        .ok_or_else(|| format!("{table} is not a table"))?
        .insert(key, toml_edit::value(value));
    Ok(())
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(get_server_settings, update_server_settings))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTINGS: ServerSettings = ServerSettings {
        port: 3100,
        upload_max_size_mb: 500,
        session_expiry_days: 30,
    };

    #[test]
    fn apply_keeps_other_items_and_comments() {
        let text = "# 手で書いたコメント\n[server]\nbind = \"127.0.0.1\" # 右のコメント\nport = 3000\n\n[log]\nfilter = \"debug\"\n";
        let updated = apply(text, &SETTINGS).expect("書き換えられなかった");
        assert!(updated.contains("# 手で書いたコメント"), "{updated}");
        assert!(
            updated.contains("bind = \"127.0.0.1\" # 右のコメント"),
            "{updated}"
        );
        assert!(updated.contains("filter = \"debug\""), "{updated}");

        let config: Config = toml::from_str(&updated).expect("書き換えた結果が読めない");
        assert_eq!(ServerSettings::from_config(&config), SETTINGS);
        assert_eq!(config.server.bind.to_string(), "127.0.0.1");
    }

    #[test]
    fn apply_to_empty_file_adds_tables() {
        let updated = apply("", &SETTINGS).expect("書き換えられなかった");
        let config: Config = toml::from_str(&updated).expect("書き換えた結果が読めない");
        assert_eq!(ServerSettings::from_config(&config), SETTINGS);
    }

    #[test]
    fn apply_rejects_a_non_table_section() {
        assert!(apply("server = 1\n", &SETTINGS).is_err());
    }

    #[test]
    fn validate_rejects_out_of_range() {
        assert!(validate(&SETTINGS).is_ok());
        for bad in [
            ServerSettings {
                port: 0,
                ..SETTINGS
            },
            ServerSettings {
                upload_max_size_mb: 0,
                ..SETTINGS
            },
            ServerSettings {
                upload_max_size_mb: MAX_UPLOAD_MAX_SIZE_MB + 1,
                ..SETTINGS
            },
            ServerSettings {
                session_expiry_days: 0,
                ..SETTINGS
            },
            ServerSettings {
                session_expiry_days: MAX_SESSION_EXPIRY_DAYS + 1,
                ..SETTINGS
            },
        ] {
            assert!(validate(&bad).is_err(), "{bad:?}");
        }
    }
}
