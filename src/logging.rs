//! ロギングの初期化。出力先(stdout / ファイル)とフィルタは設定で切り替える。
//! 管理画面からは、詳しいログ (`debug`) への切り替えとログのダウンロードができる
//! (→ `LogControl`、`api::logs`)。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry, reload};

use crate::config::{AppDirs, Config, LogConfig, LogOutput, create_owner_only_dir};

const LOG_FILE_PREFIX: &str = "weblav.log";
/// 残すログの日数 (1日1ファイル)。古いものから消す。設定項目にはしない。
const KEEP_LOG_DAYS: usize = 14;

/// `init` で登録する、プロセスに1つの切り替え。ログの購読者自体がプロセスに1つなので、合わせる。
static CONTROL: OnceLock<LogControl> = OnceLock::new();

/// 詳しいログの切り替えと、ログファイルの置き場。
///
/// 詳しいログは起動し直さずに効かせ、`config.toml` には書かない。起動し直すと、
/// 起動時のフィルタ (`RUST_LOG` か `log.filter`) に戻る。
#[derive(Clone)]
pub struct LogControl {
    inner: Arc<Inner>,
}

struct Inner {
    /// 購読者のフィルタを差し替える口。`init` を通らない (テストなど) ときは無く、切り替えは記録だけになる。
    reload: Option<reload::Handle<EnvFilter, Registry>>,
    /// `config.toml` の `log.filter`。切り替えのたびに、起動時と同じ経路 (`env_filter`) でフィルタを作り直す。
    config_filter: String,
    /// 詳しいログを出しているか。フィルタの差し替えと一緒に持ち、同時の切り替えで食い違わないようにする。
    verbose: Mutex<bool>,
    /// ログをファイルに書いていれば、その置き場。`stdout` なら無い。
    log_dir: Option<PathBuf>,
}

impl LogControl {
    /// `init` で登録したものを返す。無ければ (テストなど)、切り替えを記録だけするものを作る。
    pub fn current_or_detached(config: &Config, dirs: &AppDirs) -> Self {
        CONTROL.get().cloned().unwrap_or_else(|| {
            Self::new(None, config.log.filter.clone(), log_dir(&config.log, dirs))
        })
    }

    fn new(
        reload: Option<reload::Handle<EnvFilter, Registry>>,
        config_filter: String,
        log_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                reload,
                config_filter,
                verbose: Mutex::new(false),
                log_dir,
            }),
        }
    }

    pub fn verbose(&self) -> bool {
        *self
            .inner
            .verbose
            .lock()
            .unwrap_or_else(|err| err.into_inner())
    }

    /// 詳しいログに切り替える (`false` なら起動時のフィルタに戻す)。
    ///
    /// 詳しいログは、起動時のフィルタの既定の段階だけを `debug` に上げる。
    /// `weblav=trace` のように対象を名指しした指定は残す。
    pub fn set_verbose(&self, verbose: bool) -> Result<(), reload::Error> {
        let mut current = self
            .inner
            .verbose
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if let Some(handle) = &self.inner.reload {
            let filter = env_filter(&self.inner.config_filter);
            let filter = if verbose {
                filter.add_directive(LevelFilter::DEBUG.into())
            } else {
                filter
            };
            handle.reload(filter)?;
        }
        *current = verbose;
        Ok(())
    }

    pub fn log_dir(&self) -> Option<&Path> {
        self.inner.log_dir.as_deref()
    }
}

fn log_dir(config: &LogConfig, dirs: &AppDirs) -> Option<PathBuf> {
    (config.output == LogOutput::File).then(|| dirs.log_dir())
}

/// `dir` のログファイルを、古い日から順に返す。名前が `weblav.log.YYYY-MM-DD` なので、名前順が日付順になる。
pub fn log_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let is_log = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(LOG_FILE_PREFIX));
        if is_log && entry.file_type()?.is_file() {
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(files)
}

/// ロギングを初期化する。呼び出しには `Config` の読み込みが済んでいる必要がある
/// (出力先・フィルタが設定に依存するため)。
///
/// - フィルタは環境変数 `RUST_LOG` があればそれを、無ければ `config.filter` を使う。
/// - 返り値の `WorkerGuard` は非同期書き込みワーカーの生存期間を握っている。drop すると
///   バッファ中のログがフラッシュされずに消えるため、プロセス終了まで保持し続けること。
///   `tao` のイベントループは `run()` が `-> !` で戻らないため、停止時に明示的に drop すること
///   (`server::Running::stop`)。
pub fn init(config: &Config, dirs: &AppDirs) -> std::io::Result<WorkerGuard> {
    let log_dir = log_dir(&config.log, dirs);
    let (non_blocking, guard) = match &log_dir {
        None => tracing_appender::non_blocking(std::io::stdout()),
        Some(log_dir) => {
            create_owner_only_dir(log_dir)?;
            // 日次ローテーション: weblav.log.YYYY-MM-DD というファイル名になる。
            // 古いものは、起動時と日付が替わるときに消える。
            let file_appender = RollingFileAppender::builder()
                .rotation(Rotation::DAILY)
                .filename_prefix(LOG_FILE_PREFIX)
                .max_log_files(KEEP_LOG_DAYS)
                .build(log_dir)
                .map_err(std::io::Error::other)?;
            tracing_appender::non_blocking(file_appender)
        }
    };
    let config = &config.log;

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        // ファイル出力ではANSIエスケープは付けない。
        .with_ansi(config.output == LogOutput::Stdout);

    let (filter, reload) = reload::Layer::new(env_filter(&config.filter));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .init();
    // `init` はプロセスで1回だけ呼ばれる (購読者の登録も1回しかできない)。
    let _ = CONTROL.set(LogControl::new(
        Some(reload),
        config.filter.clone(),
        log_dir,
    ));

    // panic はデフォルトだとコンソールに出るだけで、Windowsではコンソールを切り離しているため
    // 誰にも見えない。ログにも残す(標準のフックはターミナル起動時に有用なので残したまま呼ぶ)。
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(panic = %info, "a panic occurred");
        default_hook(info);
    }));

    Ok(guard)
}

/// `RUST_LOG` があればそれを優先し、無ければ `fallback` からフィルタを作る。
/// どちらも解析できない場合は `info` にフォールバックする(ログが全く出ないよりは良い)。
fn env_filter(fallback: &str) -> EnvFilter {
    EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(fallback))
        .unwrap_or_else(|err| {
            eprintln!("failed to parse log filter '{fallback}', falling back to info: {err}");
            EnvFilter::new("info")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 切り替えは、起動し直さずに購読者のフィルタへ効く。
    #[test]
    fn set_verbose_switches_filter_without_restart() {
        // RUST_LOG があるとそちらが優先されるので、置いていない前提のテスト。
        let config_filter = "info,named_target=trace";
        let (filter, handle) = reload::Layer::new(env_filter(config_filter));
        let subscriber = tracing_subscriber::registry().with(filter);
        let control = LogControl::new(Some(handle), config_filter.to_string(), None);

        tracing::subscriber::with_default(subscriber, || {
            assert!(!tracing::enabled!(tracing::Level::DEBUG));
            control
                .set_verbose(true)
                .expect("詳しいログに切り替えられなかった");
            assert!(control.verbose());
            assert!(tracing::enabled!(tracing::Level::DEBUG));
            // 名指しした指定は debug に下げない。
            assert!(tracing::enabled!(target: "named_target", tracing::Level::TRACE));
            control
                .set_verbose(false)
                .expect("元のフィルタに戻せなかった");
            assert!(!control.verbose());
            assert!(!tracing::enabled!(tracing::Level::DEBUG));
        });
    }
}
