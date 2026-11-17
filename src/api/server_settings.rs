//! 管理画面から変える、サーバーの設定 (→ docs/access.md「ロールと操作」)。`admin` だけが読み書きできる。
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

use crate::auth::{AdminUser, ConfirmedAdmin};
use crate::config::{self, Config};
use crate::error::{AppError, AppJson, run_blocking};
use crate::state::AppState;

use super::error_detail::ValidationDetail;
use super::operations::{Op, OpRecord, changed_fields, record_or_warn};

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
    /// 共有へのファイルのアップロード1件あたりの上限 (MiB)。`None` なら上限なし。
    pub upload_max_size_mb: Option<u64>,
    /// 最後に使ってから、ログインが切れるまでの日数。0 なら期限なし。
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
    /// `config.toml` に書いていないときの値。画面が、変えてある項目に印を付け、戻せるようにするため。
    defaults: ServerSettings,
}

impl ServerSettingsResponse {
    fn new(saved: ServerSettings, running: ServerSettings) -> Self {
        Self {
            saved,
            running,
            defaults: ServerSettings::from_config(&Config::default()),
        }
    }
}

#[utoipa::path(
    get,
    path = "/admin/server-settings",
    responses(
        (status = OK, body = ServerSettingsResponse, description = "保存した値・動いている値・既定の値"),
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
    Ok(Json(ServerSettingsResponse::new(
        saved,
        state.running_server_settings,
    )))
}

#[utoipa::path(
    put,
    path = "/admin/server-settings",
    request_body = ServerSettings,
    responses(
        (status = OK, body = ServerSettingsResponse, description = "保存した後の値"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外・パスワードの確かめ直しが要る"),
        (status = 409, body = crate::error::ErrorResponse, description = "config.toml を読めない"),
        (status = 422, body = crate::error::ErrorResponse, description = "範囲の外の値"),
    )
)]
async fn update_server_settings(
    admin: ConfirmedAdmin,
    State(state): State<AppState>,
    AppJson(payload): AppJson<ServerSettings>,
) -> Result<Json<ServerSettingsResponse>, AppError> {
    validate(&payload)?;
    // ロックはファイル操作の中で外す。接続が切れてハンドラが捨てられても、書き終わるまで
    // 次の保存を待たせるため (`spawn_blocking` の処理は止まらずに続く)。
    let guard = state.config_write_lock.clone().lock_owned().await;
    let path = state.config_path.clone();
    let (before, saved) = run_blocking(move || {
        let _guard = guard;
        let before = read_saved(&path)?;
        write_saved(&path, &payload)?;
        Ok::<_, AppError>((before, read_saved(&path)?))
    })
    .await??;
    let fields = changed_fields(&before, &saved);
    if !fields.is_empty() {
        record_or_warn(
            &state.pool,
            &OpRecord::new(Op::UpdateServerSettings, &admin).fields(fields),
        )
        .await;
    }
    tracing::info!(
        ?payload,
        "server settings saved; effective from the next start"
    );
    Ok(Json(ServerSettingsResponse::new(
        saved,
        state.running_server_settings,
    )))
}

/// ブラウザーが接続を断るポート。WHATWG Fetch の「bad port」の表をそのまま写した
/// (https://fetch.spec.whatwg.org/#port-blocking 。2026-11-14 の版)。主なブラウザーはこの表に従って断る。
/// 0 は前の検査で断るので含めない。
pub(crate) const BROWSER_BLOCKED_PORTS: &[u16] = &[
    1, 7, 9, 11, 13, 15, 17, 19, 20, 21, 22, 23, 25, 37, 42, 43, 53, 69, 77, 79, 87, 95, 101, 102,
    103, 104, 109, 110, 111, 113, 115, 117, 119, 123, 135, 137, 139, 143, 161, 179, 389, 427, 465,
    512, 513, 514, 515, 526, 530, 531, 532, 540, 548, 554, 556, 563, 587, 601, 636, 989, 990, 993,
    995, 1719, 1720, 1723, 2049, 3659, 4045, 4190, 5060, 5061, 6000, 6566, 6665, 6666, 6667, 6668,
    6669, 6679, 6697, 10080,
];

fn validate(settings: &ServerSettings) -> Result<(), AppError> {
    if settings.port == 0 {
        return Err(AppError::Validation("port must be 1 or greater".into()));
    }
    if BROWSER_BLOCKED_PORTS.contains(&settings.port) {
        return Err(AppError::ValidationDetailed {
            message: format!("browsers refuse to open port {}", settings.port),
            detail: ValidationDetail::PortBlockedByBrowsers {
                port: settings.port,
            },
        });
    }
    if settings
        .upload_max_size_mb
        .is_some_and(|mb| !(1..=MAX_UPLOAD_MAX_SIZE_MB).contains(&mb))
    {
        return Err(AppError::Validation(format!(
            "uploadMaxSizeMb must be between 1 and {MAX_UPLOAD_MAX_SIZE_MB}"
        )));
    }
    if !(0..=MAX_SESSION_EXPIRY_DAYS).contains(&settings.session_expiry_days) {
        return Err(AppError::Validation(format!(
            "sessionExpiryDays must be between 0 and {MAX_SESSION_EXPIRY_DAYS}"
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
    config::edit_file(path, |doc| apply(doc, settings)).map_err(|err| match err {
        config::EditError::Io(err) => AppError::Io(err),
        config::EditError::Broken(err) => broken_config(err),
    })
}

/// 該当する項目だけを書き換える。表 (`[server]` など) が無ければ足す。
fn apply(doc: &mut toml_edit::DocumentMut, settings: &ServerSettings) -> Result<(), String> {
    set(doc, "server", "port", i64::from(settings.port))?;
    // 上限は `validate` で i64 に収まる範囲に絞ってある。上限なしは、項目を消して既定 (上限なし) に任せる。
    match settings.upload_max_size_mb {
        Some(mb) => set(doc, "upload", "max_size_mb", mb as i64)?,
        None => remove(doc, "upload", "max_size_mb"),
    }
    set(doc, "session", "expiry_days", settings.session_expiry_days)
}

fn set(doc: &mut toml_edit::DocumentMut, table: &str, key: &str, value: i64) -> Result<(), String> {
    doc.entry(table)
        .or_insert(toml_edit::table())
        .as_table_like_mut()
        .ok_or_else(|| format!("{table} is not a table"))?
        .insert(key, toml_edit::value(value));
    Ok(())
}

/// 表 (`[upload]` など) に項目があれば消す。表が無ければ何もしない (空の表を足さない)。
fn remove(doc: &mut toml_edit::DocumentMut, table: &str, key: &str) {
    if let Some(table) = doc.get_mut(table).and_then(|item| item.as_table_like_mut()) {
        table.remove(key);
    }
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(get_server_settings, update_server_settings))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTINGS: ServerSettings = ServerSettings {
        port: 3100,
        upload_max_size_mb: Some(500),
        session_expiry_days: 30,
    };

    /// `text` を読み、`settings` を書き込んだ結果の文字列。
    fn apply_to(text: &str, settings: &ServerSettings) -> Result<String, String> {
        let mut doc: toml_edit::DocumentMut = text.parse().map_err(|err| format!("{err}"))?;
        apply(&mut doc, settings)?;
        Ok(doc.to_string())
    }

    #[test]
    fn apply_keeps_other_items_and_comments() {
        let text = "# 手で書いたコメント\n[server]\nbind = \"127.0.0.1\" # 右のコメント\nport = 8370\n\n[log]\nfilter = \"debug\"\n";
        let updated = apply_to(text, &SETTINGS).expect("書き換えられなかった");
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
        let updated = apply_to("", &SETTINGS).expect("書き換えられなかった");
        let config: Config = toml::from_str(&updated).expect("書き換えた結果が読めない");
        assert_eq!(ServerSettings::from_config(&config), SETTINGS);
    }

    #[test]
    fn apply_removes_the_upload_cap_when_unlimited() {
        let text = "[upload]\nmax_size_mb = 100\n";
        let unlimited = ServerSettings {
            upload_max_size_mb: None,
            ..SETTINGS
        };
        let updated = apply_to(text, &unlimited).expect("書き換えられなかった");
        assert!(!updated.contains("max_size_mb"), "{updated}");
        let config: Config = toml::from_str(&updated).expect("書き換えた結果が読めない");
        assert_eq!(ServerSettings::from_config(&config), unlimited);
        assert!(validate(&unlimited).is_ok());
    }

    #[test]
    fn apply_rejects_a_non_table_section() {
        assert!(apply_to("server = 1\n", &SETTINGS).is_err());
    }

    #[test]
    fn validate_rejects_out_of_range() {
        assert!(validate(&SETTINGS).is_ok());
        assert!(
            validate(&ServerSettings {
                session_expiry_days: 0,
                ..SETTINGS
            })
            .is_ok()
        );
        for bad in [
            ServerSettings {
                port: 0,
                ..SETTINGS
            },
            ServerSettings {
                port: 6000,
                ..SETTINGS
            },
            ServerSettings {
                port: 10080,
                ..SETTINGS
            },
            ServerSettings {
                upload_max_size_mb: Some(0),
                ..SETTINGS
            },
            ServerSettings {
                upload_max_size_mb: Some(MAX_UPLOAD_MAX_SIZE_MB + 1),
                ..SETTINGS
            },
            ServerSettings {
                session_expiry_days: -1,
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
