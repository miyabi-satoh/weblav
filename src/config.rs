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
/// 開発・e2e で置き場所を分けたいときや、配置場所を固定したい運用向け。
pub const HOME_ENV: &str = "WEBLAV_HOME";

const CONFIG_FILE_NAME: &str = "config.toml";
const DB_FILE_NAME: &str = "weblav.db";
const SESSION_KEY_FILE_NAME: &str = "session.key";
const LOG_DIR_NAME: &str = "logs";
const THUMBNAILS_DIR_NAME: &str = "thumbnails";
const LINK_PREVIEWS_DIR_NAME: &str = "link-previews";
const BACKUP_WORK_DIR_NAME: &str = "backup-work";
const LOCK_FILE_NAME: &str = "weblav.lock";
const ADDR_FILE_NAME: &str = "weblav.addr";

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

    /// weblav 自身の置き場 (設定とデータ) として、共有から外すパス (→ docs/folders.md「WebLAV 自身の置き場」)。
    ///
    /// Store 版 (MSIX) では、`%APPDATA%`・`%LOCALAPPDATA%` に書いたものはパッケージごとの場所に回され
    /// (→ docs/distribution.md「設定とデータの置き場所」)、同じ中身が2つのパスから見える。
    /// `C:\` やユーザーのフォルダーを共有にしたとき、回された先のパスからも出ないよう、そちらも足す。
    pub fn own_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.config_dir.clone(), self.data_dir.clone()];
        if let Some(redirect) = PackageRedirect::current() {
            let redirected: Vec<PathBuf> = dirs
                .iter()
                .filter_map(|dir| redirect.redirected(dir))
                .collect();
            dirs.extend(redirected);
        }
        dirs
    }
}

/// MSIX のパッケージの中で動いているときの、`AppData` の書き込みが回される先
/// (`%LOCALAPPDATA%\Packages\<パッケージファミリー名>\LocalCache\{Roaming,Local}`)。
// Windows 以外では作られない。回し先の決め方は OS によらないので、テストはどこでも流す。
#[cfg_attr(not(windows), allow(dead_code))]
struct PackageRedirect {
    roaming: PathBuf,
    local: PathBuf,
    family_name: String,
}

impl PackageRedirect {
    /// パッケージの外 (開発中の exe・Windows 以外) では `None`。
    #[cfg(windows)]
    fn current() -> Option<Self> {
        let family_name = current_package_family_name()?;
        let base = directories::BaseDirs::new()?;
        Some(Self {
            roaming: base.data_dir().to_path_buf(),
            local: base.data_local_dir().to_path_buf(),
            family_name,
        })
    }

    #[cfg(not(windows))]
    fn current() -> Option<Self> {
        None
    }

    /// `dir` が `%APPDATA%`・`%LOCALAPPDATA%` の中なら、回された先のパス。外なら `None`。
    fn redirected(&self, dir: &Path) -> Option<PathBuf> {
        let cache = self
            .local
            .join("Packages")
            .join(&self.family_name)
            .join("LocalCache");
        if let Ok(rest) = dir.strip_prefix(&self.roaming) {
            return Some(cache.join("Roaming").join(rest));
        }
        if let Ok(rest) = dir.strip_prefix(&self.local) {
            // 既に回された先のパスなら、重ねて足さない。
            if dir.starts_with(&cache) {
                return None;
            }
            return Some(cache.join("Local").join(rest));
        }
        None
    }
}

/// 今のプロセスのパッケージファミリー名。パッケージの外なら `None`。
#[cfg(windows)]
fn current_package_family_name() -> Option<String> {
    use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
    use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFamilyName;
    use windows::core::PWSTR;

    // 1回目で要る長さ (終端の NUL を含む) を受け取り、2回目で中身を受け取る。
    let mut len = 0u32;
    if unsafe { GetCurrentPackageFamilyName(&mut len, None) } != ERROR_INSUFFICIENT_BUFFER {
        return None;
    }
    let mut buf = vec![0u16; len as usize];
    if unsafe { GetCurrentPackageFamilyName(&mut len, Some(PWSTR(buf.as_mut_ptr()))) }
        != ERROR_SUCCESS
    {
        return None;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16(&buf[..end]).ok()
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
            // 理由は `config.example.toml` の port コメント参照
            port: 8370,
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
    /// 最終アクセスからこの日数が経過したセッションを失効させる。0 なら期限なし (`session::layer`)。
    /// 負数は、セッションが作成前に失効した扱いになってしまうため許可しない。
    #[serde(deserialize_with = "deserialize_non_negative_i64")]
    pub expiry_days: i64,
}

fn deserialize_non_negative_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = i64::deserialize(deserializer)?;
    if value < 0 {
        return Err(serde::de::Error::custom(format!(
            "expiry_days must be 0 or greater (got: {value})"
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

/// 共有へのファイルのアップロードの設定。
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UploadConfig {
    /// 1ファイルあたりの最大サイズ(MiB)。ストリーミング中に厳密にこの値でチェックする
    /// (multipartボディ全体に掛けるサーバー側の受付上限は、これに加えて区切りの分の
    /// 余裕を持たせた値を別途使う。`AppState::max_upload_bytes`・`shares::write::router`参照)。
    /// 既定は上限なし (`None`)。書き込むのは共有の中で、ディスクの空きは置く前に確かめるため。
    /// 0 も上限なしと読む。管理画面が 0 を上限なしとして見せるので、手で書いたときも揃える。
    #[serde(deserialize_with = "deserialize_zero_as_none")]
    pub max_size_mb: Option<u64>,
}

fn deserialize_zero_as_none<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<u64>::deserialize(deserializer)?.filter(|&value| value != 0))
}

impl UploadConfig {
    /// 1ファイルあたりの最大バイト数。上限なしなら `None`。
    pub fn max_bytes(&self) -> Option<u64> {
        self.max_size_mb.map(|mb| mb.saturating_mul(1024 * 1024))
    }
}

/// タスクトレイの設定 (→ `tray`)。
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TrayConfig {
    /// 管理者がまだいないときに、起動したらセットアップの画面を開くか (→ docs/access.md「初回セットアップ」)。
    /// 切れるのは、配る形のアプリを何度も起動する自動の確認 (`e2e/`) が、そのたびにブラウザーを開かないようにするため。
    pub open_setup_on_launch: bool,
    /// トレイのメニューの表示言語 (→ docs/architecture.md「タスクトレイの表示言語」)。トレイのメニューで選ぶ。
    pub language: TrayLanguage,
}

impl Default for TrayConfig {
    fn default() -> Self {
        Self {
            open_setup_on_launch: true,
            language: TrayLanguage::System,
        }
    }
}

/// トレイのメニューの表示言語の選び方。`System` は OS の表示言語に合わせる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrayLanguage {
    System,
    Ja,
    En,
}

impl TrayLanguage {
    /// `config.toml` に書く値。
    pub fn as_str(self) -> &'static str {
        match self {
            TrayLanguage::System => "system",
            TrayLanguage::Ja => "ja",
            TrayLanguage::En => "en",
        }
    }
}

/// `config.toml` の `[tray] language` を書き換える。既定 (`system`) は項目を消して既定に任せる。
pub fn write_tray_language(path: &Path, language: TrayLanguage) -> Result<(), EditError> {
    edit_file(path, |doc| {
        if language == TrayLanguage::System {
            remove_value(doc, "tray", "language");
            Ok(())
        } else {
            set_value(doc, "tray", "language", language.as_str())
        }
    })
}

/// `edit_file` の中で、表 (`[server]` など) の項目を書く。表が無ければ足す。
pub fn set_value(
    doc: &mut toml_edit::DocumentMut,
    table: &str,
    key: &str,
    value: impl Into<toml_edit::Value>,
) -> Result<(), String> {
    doc.entry(table)
        .or_insert(toml_edit::table())
        .as_table_like_mut()
        .ok_or_else(|| format!("{table} is not a table"))?
        .insert(key, toml_edit::value(value));
    Ok(())
}

/// `edit_file` の中で、表 (`[upload]` など) に項目があれば消す。表が無ければ何もしない (空の表を足さない)。
pub fn remove_value(doc: &mut toml_edit::DocumentMut, table: &str, key: &str) {
    if let Some(table) = doc.get_mut(table).and_then(|item| item.as_table_like_mut()) {
        table.remove(key);
    }
}

/// `edit_file` の失敗。
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// 今の内容か、書き換えた後の内容を、設定として読めない。
    #[error("config.toml cannot be read: {0}")]
    Broken(String),
}

/// `config.toml` を `edit` で書き換える。手で書いたほかの項目やコメントは残す。
/// 書き換えた結果を起動のときと同じ読み方で確かめてから置き換える。読めない形を書くと、次に起動できなくなるため。
/// 今の内容が読めない (起動した後に手で書き換えた等) ときは書き換えない。手で書いた内容を消さないため。
/// 同じプロセスの中で、書き換えを1つずつにする。トレイとサイト設定の保存が重なると、
/// 片方の書き換えが消え、隣の一時ファイルも取り合うため。
pub fn edit_file(
    path: &Path,
    edit: impl FnOnce(&mut toml_edit::DocumentMut) -> Result<(), String>,
) -> Result<(), EditError> {
    static EDIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // 前の書き換えが途中で panic しても、ファイルは置き換えの前か後のどちらかなので続けてよい。
    let _guard = EDIT_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(EditError::Io(err)),
    };
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|err| EditError::Broken(format!("{err}")))?;
    edit(&mut doc).map_err(EditError::Broken)?;
    let updated = doc.to_string();
    toml::from_str::<Config>(&updated).map_err(|err| EditError::Broken(format!("{err}")))?;

    if let Some(dir) = path.parent() {
        create_owner_only_dir(dir)?;
    }
    // シンボリックリンクなら、リンクを普通のファイルに置き換えず、リンク先を書き換える。
    let target = match std::fs::canonicalize(path) {
        Ok(target) => target,
        // リンク先がまだ無いリンクは辿れないので、リンクが指す先を読んでそこに作る。
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => match std::fs::read_link(path) {
            Ok(link) => path.parent().map_or(link.clone(), |dir| dir.join(&link)),
            Err(_) => path.to_path_buf(),
        },
        Err(err) => return Err(EditError::Io(err)),
    };
    // 途中で落ちても壊れた config.toml を残さないよう、隣に書いてから置き換える。
    // `session.secret` を書けるファイルなので、新しく作るときは所有者だけが読めるようにし、
    // 元があればその権限を引き継ぐ。
    replace_owner_only_file(
        &target.with_extension("toml.tmp"),
        &target,
        updated.as_bytes(),
        true,
    )?;
    Ok(())
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub server: ServerConfig,
    pub log: LogConfig,
    pub session: SessionConfig,
    pub upload: UploadConfig,
    pub tray: TrayConfig,
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
    fn package_redirect_maps_appdata_to_the_package_local_cache() {
        let redirect = PackageRedirect {
            roaming: PathBuf::from("/Users/u/AppData/Roaming"),
            local: PathBuf::from("/Users/u/AppData/Local"),
            family_name: "amiiby.WebLAV_tv82n7df3ay6j".into(),
        };
        let cache =
            PathBuf::from("/Users/u/AppData/Local/Packages/amiiby.WebLAV_tv82n7df3ay6j/LocalCache");
        assert_eq!(
            redirect.redirected(Path::new("/Users/u/AppData/Roaming/amiiby/weblav/config")),
            Some(cache.join("Roaming/amiiby/weblav/config"))
        );
        assert_eq!(
            redirect.redirected(Path::new("/Users/u/AppData/Local/amiiby/weblav/data")),
            Some(cache.join("Local/amiiby/weblav/data"))
        );
        // `WEBLAV_HOME` のように `AppData` の外なら回されない。既に回された先なら重ねない。
        assert_eq!(redirect.redirected(Path::new("/srv/weblav")), None);
        assert_eq!(
            redirect.redirected(&cache.join("Local/amiiby/weblav/data")),
            None
        );
    }

    #[test]
    fn example_file_parses_to_defaults() {
        let text = include_str!("../config.example.toml");
        let config: Config = toml::from_str(text).expect("config.example.toml should parse");
        assert_eq!(config.server.bind, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        assert_eq!(config.server.port, 8370);
        assert_eq!(config.log.filter, "info");
        assert_eq!(config.log.output, LogOutput::File);
        assert_eq!(config.session.secret, "");
        assert!(!config.session.secure_cookie);
        assert_eq!(config.session.expiry_days, 14);
        assert_eq!(config.upload.max_size_mb, None);
        assert!(config.tray.open_setup_on_launch);
    }

    /// e2e が書く設定 (`e2e/lib/*.mjs`) で、起動したときにセットアップを開かなくなる。
    #[test]
    fn opening_setup_on_launch_can_be_turned_off() {
        let config: Config =
            toml::from_str("[tray]\nopen_setup_on_launch = false\n").expect("設定を読めるはず");
        assert!(!config.tray.open_setup_on_launch);
    }

    /// トレイで選んだ表示言語を書き、手で書いたほかの項目とコメントは残す。「システム」に戻したら項目を消す。
    #[test]
    fn tray_language_is_written_and_removed() {
        let tmp = TempDir::new("tray_language_is_written_and_removed");
        let path = tmp.path().join("config.toml");
        std::fs::write(
            &path,
            "# 手で書いたコメント\n[tray]\nopen_setup_on_launch = false\n",
        )
        .expect("ファイルを書けなかった");

        write_tray_language(&path, TrayLanguage::En).expect("書けなかった");
        let text = std::fs::read_to_string(&path).expect("読めなかった");
        assert!(text.contains("# 手で書いたコメント"), "{text}");
        let config = Config::load_from(&path).expect("設定を読めるはず");
        assert_eq!(config.tray.language, TrayLanguage::En);
        assert!(!config.tray.open_setup_on_launch);

        write_tray_language(&path, TrayLanguage::System).expect("書けなかった");
        let text = std::fs::read_to_string(&path).expect("読めなかった");
        assert!(!text.contains("language"), "{text}");
        assert!(text.contains("open_setup_on_launch = false"), "{text}");
    }

    /// 読めない `config.toml` は書き換えない。手で書いた内容を消さないため。
    #[test]
    fn tray_language_is_not_written_into_a_broken_file() {
        let tmp = TempDir::new("tray_language_is_not_written_into_a_broken_file");
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "[server]\nport = \"abc\"\n").expect("ファイルを書けなかった");

        let err = write_tray_language(&path, TrayLanguage::Ja).expect_err("書けてはいけない");
        assert!(matches!(err, EditError::Broken(_)), "{err:?}");
        let text = std::fs::read_to_string(&path).expect("読めなかった");
        assert_eq!(text, "[server]\nport = \"abc\"\n");
    }

    #[test]
    fn empty_input_uses_defaults() {
        let config: Config = toml::from_str("").expect("empty input should use defaults");
        assert_eq!(config.server.socket_addr().port(), 8370);
        assert_eq!(config.log.output, LogOutput::File);
    }

    #[test]
    fn partial_server_keeps_bind_default() {
        let config: Config = toml::from_str("[server]\nport = 1\n").expect("設定を読めるはず");
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
        let config =
            Config::load_from(Path::new("does/not/exist/config.toml")).expect("設定を読めるはず");
        assert_eq!(config.server.port, 8370);
    }

    #[test]
    fn parse_error_keeps_line_number_detail() {
        let tmp = TempDir::new("parse_error_keeps_line_number_detail");
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "[server]\nport = \"abc\"\n").expect("ファイルを書けなかった");

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
    fn zero_expiry_days_is_accepted_as_no_expiry() {
        let config: Config =
            toml::from_str("[session]\nexpiry_days = 0\n").expect("0 を読めなかった");
        assert_eq!(config.session.expiry_days, 0);
    }

    #[test]
    fn zero_upload_max_size_mb_means_no_limit() {
        let config: Config =
            toml::from_str("[upload]\nmax_size_mb = 0\n").expect("0 を読めなかった");
        assert_eq!(config.upload.max_size_mb, None);
    }

    #[test]
    fn negative_expiry_days_is_rejected() {
        let result: Result<Config, _> = toml::from_str("[session]\nexpiry_days = -1\n");
        assert!(result.is_err());
    }
}
