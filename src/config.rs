//! 設定ファイル(`config.toml`)の読み込みと、設定・データを置くディレクトリの解決。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::Deserialize;

/// `directories::ProjectDirs::from` の3引数。
/// macOS: `com.amiiby.weblav` / Windows: `amiiby\weblav` / Linux: `weblav` に解決される。
/// 一度リリースした後に変更すると、既存ユーザーの設定・DBファイルの配置場所が変わってしまうため注意。
const APP_QUALIFIER: &str = "com";
const APP_ORGANIZATION: &str = "amiiby";
const APP_APPLICATION: &str = "weblav";

/// この環境変数が設定されていれば、設定・データ・ログをすべてそのディレクトリ直下に置く。
/// Docker / systemd のように HOME が無い、あるいは配置場所を固定したい運用向け。
pub const HOME_ENV: &str = "WEBLAV_HOME";

const CONFIG_FILE_NAME: &str = "config.toml";
const DB_FILE_NAME: &str = "weblav.db";
const SESSION_KEY_FILE_NAME: &str = "session.key";
const LOG_DIR_NAME: &str = "logs";
const BLOBS_DIR_NAME: &str = "blobs";
const THUMBNAILS_DIR_NAME: &str = "thumbnails";
const LINK_PREVIEWS_DIR_NAME: &str = "link-previews";
const BACKUP_WORK_DIR_NAME: &str = "backup-work";
const LOCK_FILE_NAME: &str = "weblav.lock";
const ADDR_FILE_NAME: &str = "weblav.addr";
const PRO_FILE_NAME: &str = "pro.json";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to resolve config/data directory (set {HOME_ENV} if HOME is unavailable)")]
    NoHomeDir,
    #[error("failed to read config file: {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config file: {path}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

/// 設定ファイル・DB・セッション鍵・ログの置き場所。
#[derive(Debug, Clone)]
pub struct AppDirs {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl AppDirs {
    /// 1. `WEBLAV_HOME` が設定されていれば config_dir = data_dir = その値
    /// 2. それ以外は、そのユーザーのアプリデータディレクトリ(`ProjectDirs`、→ docs/distribution.md「常駐 (Windows)」)
    pub fn resolve() -> Result<Self, Error> {
        if let Some(home) = std::env::var_os(HOME_ENV).filter(|v| !v.is_empty()) {
            let home = PathBuf::from(home);
            return Ok(Self {
                config_dir: home.clone(),
                data_dir: home,
            });
        }

        let dirs = ProjectDirs::from(APP_QUALIFIER, APP_ORGANIZATION, APP_APPLICATION)
            .ok_or(Error::NoHomeDir)?;
        Ok(Self {
            config_dir: dirs.config_dir().to_path_buf(),
            data_dir: dirs.data_local_dir().to_path_buf(),
        })
    }

    pub fn config_path(&self) -> PathBuf {
        self.config_dir.join(CONFIG_FILE_NAME)
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join(DB_FILE_NAME)
    }

    pub fn session_key_path(&self) -> PathBuf {
        self.data_dir.join(SESSION_KEY_FILE_NAME)
    }

    pub fn log_dir(&self) -> PathBuf {
        self.data_dir.join(LOG_DIR_NAME)
    }

    /// アップロードされたファイルコンテンツの実体(content-addressed blobストレージ)を
    /// 置くディレクトリ。作成自体は最初の書き込み時に行う(DB・セッション鍵と同じ、
    /// 遅延作成の方針)。
    pub fn blobs_dir(&self) -> PathBuf {
        self.data_dir.join(BLOBS_DIR_NAME)
    }

    /// 画像の縮小画像を置くディレクトリ (→ `api::thumbnails`)。元のファイルから作り直せるので、消してもよい。
    pub fn thumbnails_dir(&self) -> PathBuf {
        self.data_dir.join(THUMBNAILS_DIR_NAME)
    }

    /// リンクのカードの画像とアイコンを置くディレクトリ (→ `api::link_preview`)。また取りに行けるので、消してもよい。
    pub fn link_previews_dir(&self) -> PathBuf {
        self.data_dir.join(LINK_PREVIEWS_DIR_NAME)
    }

    /// バックアップを作る・戻すときの作業用の置き場 (→ `api::backup`)。DB の写しを含むので、
    /// OS の一時ディレクトリではなくここに置く。起動したときに中身を消す。
    pub fn backup_work_dir(&self) -> PathBuf {
        self.data_dir.join(BACKUP_WORK_DIR_NAME)
    }

    /// シングルインスタンス化(`single_instance` モジュール)用のロックファイルの場所。
    pub fn lock_path(&self) -> PathBuf {
        self.data_dir.join(LOCK_FILE_NAME)
    }

    /// 動いているサーバーの実際の待ち受けのアドレスを書くファイル (→ `server::AlreadyRunning`)。
    pub fn addr_path(&self) -> PathBuf {
        self.data_dir.join(ADDR_FILE_NAME)
    }

    /// Pro の結び付きと許可 (→ `crate::pro`)。バックアップには入れない (→ docs/pro.md「結び付きと許可」)。
    pub fn pro_path(&self) -> PathBuf {
        self.data_dir.join(PRO_FILE_NAME)
    }
}

/// ディレクトリを作成する。Unixでは所有者だけが読み書きできる権限(0700)にする
/// (DB・セッション鍵・ログなど、機微な情報の置き場所として使うため)。
/// 既に存在するディレクトリに対して呼んだ場合も権限を0700に揃える。
pub fn create_owner_only_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// 所有者のみ読み書き可能(unix: 0600)なパーミッションで新規ファイルを作成する。
/// 権限を作った時点で決めるため、既にあるファイルは開かず `create_new` で新しく作る。
#[cfg(unix)]
fn create_owner_only_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_owner_only_file(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::options()
        .write(true)
        .create_new(true)
        .open(path)
}

/// `bytes` を隣の一時ファイル `tmp` に書いてから、`target` へ rename で置き換える。
/// 途中で落ちても書きかけの `target` を残さないため (rename は同一ファイルシステム上で atomic)。
/// 一時ファイルは作った時点で所有者だけが読める権限にする (作ってから絞ると、その間 umask の権限で晒されるため)。
/// `keep_target_permissions` なら、置き換える前に `target` の権限を引き継ぐ (手で絞った権限を戻さないため)。
pub fn replace_owner_only_file(
    tmp: &Path,
    target: &Path,
    bytes: &[u8],
    keep_target_permissions: bool,
) -> std::io::Result<()> {
    use std::io::Write;

    // 前に落ちたときの一時ファイルが残っていると `create_new` で作れないため、先に消す。
    let _ = std::fs::remove_file(tmp);
    let result = (|| {
        let mut file = create_owner_only_file(tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        if keep_target_permissions && let Ok(metadata) = std::fs::metadata(target) {
            std::fs::set_permissions(tmp, metadata.permissions())?;
        }
        std::fs::rename(tmp, target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub bind: IpAddr,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            // 理由は `config.example.toml` の bind コメント参照
            bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 3000,
        }
    }
}

impl ServerConfig {
    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.bind, self.port)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogOutput {
    #[default]
    Stdout,
    /// `AppDirs::log_dir()` 配下に日次ローテーションで出力する。
    File,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LogConfig {
    /// `tracing_subscriber::EnvFilter` の書式。環境変数 `RUST_LOG` があればそちらを優先する。
    pub filter: String,
    pub output: LogOutput,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            filter: "info".to_string(),
            // Windowsではコンソールを切り離しているため、既定はファイル出力必須。
            output: LogOutput::File,
        }
    }
}

// secret はログに出したくないため Debug を手書きし、値を伏せる。
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SessionConfig {
    /// セッションCookieの署名鍵(base64エンコードされた64バイト)。
    /// 空なら `AppDirs::session_key_path()` に自動生成・永続化する。
    pub secret: String,
    /// セッションCookieにSecure属性を付与するか。HTTPS終端がある場合にtrueにする。
    pub secure_cookie: bool,
    /// 最終アクセスからこの日数が経過したセッションを失効させる。1以上の値のみ許可する
    /// (0や負数だとセッションが即座に、または作成前に失効した扱いになってしまうため)。
    #[serde(deserialize_with = "deserialize_positive_i64")]
    pub expiry_days: i64,
}

fn deserialize_positive_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = i64::deserialize(deserializer)?;
    if value <= 0 {
        return Err(serde::de::Error::custom(format!(
            "expiry_days must be 1 or greater (got: {value})"
        )));
    }
    Ok(value)
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            secret: String::new(),
            // ループバックHTTP運用を既定の用途と想定しているため無効。
            secure_cookie: false,
            expiry_days: 14,
        }
    }
}

impl std::fmt::Debug for SessionConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionConfig")
            .field("secret", &"<redacted>")
            .field("secure_cookie", &self.secure_cookie)
            .field("expiry_days", &self.expiry_days)
            .finish()
    }
}

/// ファイルアップロード型コンテンツ(type='file')の設定。
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UploadConfig {
    /// 1ファイルあたりの最大サイズ(MiB)。ストリーミング中に厳密にこの値でチェックする
    /// (multipartボディ全体に掛けるサーバー側の受付上限は、これに加えてテキストフィールド分の
    /// 余裕を持たせた値を別途使う。`AppState::max_upload_bytes`・`contents::router`参照)。
    pub max_size_mb: u64,
}

impl Default for UploadConfig {
    fn default() -> Self {
        Self { max_size_mb: 100 }
    }
}

impl UploadConfig {
    pub fn max_bytes(&self) -> u64 {
        self.max_size_mb.saturating_mul(1024 * 1024)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub server: ServerConfig,
    pub log: LogConfig,
    pub session: SessionConfig,
    pub upload: UploadConfig,
}

impl Config {
    /// `dirs.config_path()` を読み込む。
    /// ファイルが存在しない場合は `Config::default()` を返す(初回起動時に無設定で動くように)。
    pub fn load(dirs: &AppDirs) -> Result<Self, Error> {
        Self::load_from(&dirs.config_path())
    }

    pub(crate) fn load_from(path: &Path) -> Result<Self, Error> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(source) => {
                return Err(Error::Read {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };

        toml::from_str(&text).map_err(|source| Error::Parse {
            path: path.to_path_buf(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn example_file_parses_to_defaults() {
        let text = include_str!("../config.example.toml");
        let config: Config = toml::from_str(text).expect("config.example.toml should parse");
        assert_eq!(config.server.bind, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        assert_eq!(config.server.port, 3000);
        assert_eq!(config.log.filter, "info");
        assert_eq!(config.log.output, LogOutput::File);
        assert_eq!(config.session.secret, "");
        assert!(!config.session.secure_cookie);
        assert_eq!(config.session.expiry_days, 14);
        assert_eq!(config.upload.max_size_mb, 100);
    }

    #[test]
    fn empty_input_uses_defaults() {
        let config: Config = toml::from_str("").expect("empty input should use defaults");
        assert_eq!(config.server.socket_addr().port(), 3000);
        assert_eq!(config.log.output, LogOutput::File);
    }

    #[test]
    fn partial_server_keeps_bind_default() {
        let config: Config = toml::from_str("[server]\nport = 1\n").unwrap();
        assert_eq!(config.server.bind, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        assert_eq!(config.server.port, 1);
    }

    #[test]
    fn unknown_field_is_rejected() {
        let result: Result<Config, _> = toml::from_str("[server]\nprot = 8080\n");
        assert!(result.is_err());
    }

    #[test]
    fn unknown_log_output_is_rejected() {
        let result: Result<Config, _> = toml::from_str("[log]\noutput = \"syslog\"\n");
        assert!(result.is_err());
    }

    #[test]
    fn missing_file_falls_back_to_default() {
        let config = Config::load_from(Path::new("does/not/exist/config.toml")).unwrap();
        assert_eq!(config.server.port, 3000);
    }

    #[test]
    fn parse_error_keeps_line_number_detail() {
        let tmp = TempDir::new("parse_error_keeps_line_number_detail");
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "[server]\nport = \"abc\"\n").unwrap();

        let err = Config::load_from(&path).expect_err("invalid port should fail to parse");
        match &err {
            Error::Parse { source, .. } => {
                assert!(source.to_string().contains("line 2"), "{source}");
            }
            other => panic!("expected Error::Parse, got {other:?}"),
        }
    }

    #[test]
    fn app_dirs_derive_paths_from_data_dir() {
        let dirs = AppDirs {
            config_dir: PathBuf::from("/cfg"),
            data_dir: PathBuf::from("/data"),
        };
        assert_eq!(dirs.config_path(), Path::new("/cfg/config.toml"));
        assert_eq!(dirs.db_path(), Path::new("/data/weblav.db"));
        assert_eq!(dirs.session_key_path(), Path::new("/data/session.key"));
        assert_eq!(dirs.log_dir(), Path::new("/data/logs"));
        assert_eq!(dirs.blobs_dir(), Path::new("/data/blobs"));
        assert_eq!(dirs.lock_path(), Path::new("/data/weblav.lock"));
        assert_eq!(dirs.addr_path(), Path::new("/data/weblav.addr"));
    }

    #[test]
    fn session_config_debug_redacts_secret() {
        let config = SessionConfig {
            secret: "super-secret".to_string(),
            ..Default::default()
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("super-secret"), "{debug}");
        assert!(debug.contains("<redacted>"), "{debug}");
    }

    #[test]
    fn zero_expiry_days_is_rejected() {
        let result: Result<Config, _> = toml::from_str("[session]\nexpiry_days = 0\n");
        assert!(result.is_err());
    }

    #[test]
    fn negative_expiry_days_is_rejected() {
        let result: Result<Config, _> = toml::from_str("[session]\nexpiry_days = -1\n");
        assert!(result.is_err());
    }
}
