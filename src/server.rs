//! サーバー本体の起動と停止。トレイ (`weblav`) と、サーバーだけを動かす `weblav-service` の両方から使う。

use std::fs::File;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use tower_sessions::cookie::Key;
use tower_sessions::session_store::ExpiredDeletion;
use tower_sessions_sqlx_store::SqliteStore;
use tracing_appender::non_blocking::WorkerGuard;

use crate::config::{self, AppDirs, Config};
use crate::error::error_chain;
use crate::folder_picker::FolderPicker;
use crate::{build_app, db, logging, session, single_instance};

/// 停止を指示してから、処理中の応答を待つ上限。動画の再生などで接続が続くと、いつまでも止まらないため。
const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);
/// 停止のとき、DB の接続が返ってくるのを待つ上限。
const POOL_CLOSE_GRACE: Duration = Duration::from_secs(5);
/// 停止のとき、ランタイムに残ったタスクを待つ上限。
const RUNTIME_SHUTDOWN_GRACE: Duration = Duration::from_secs(1);

/// 設定のポートが使えなかったときに、上へずらして試す数。
const PORT_FALLBACK_ATTEMPTS: u16 = 10;

/// 設定のポートが使えなかったときの扱い (→ docs/distribution.md「常駐 (Windows)」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnPortUnavailable {
    /// 起動に失敗する。開発や e2e で動かすときに、黙って別のポートで動かないように。
    Fail,
    /// 上の番号へずらして起動する。トレイ版で使う。
    TryNext,
}

/// 起動の失敗。呼び出し元 (トレイ・コンソール) ごとに伝え方が違うので、文言だけを返す。
/// ログの初期化より後の失敗は、返す前にログへ書き出し済み。
#[derive(Debug)]
pub struct StartupError {
    pub message: String,
    /// 同じ置き場所を使うサーバーが既に動いていたとき。
    /// トレイは、これがあればエラーを出す代わりに、動いているほうの画面をブラウザーで開く。
    pub already_running: Option<AlreadyRunning>,
}

/// 既に動いているサーバーの待ち受けのアドレスを知る手掛かり。
#[derive(Debug)]
pub struct AlreadyRunning {
    addr_path: PathBuf,
    /// 設定の待ち受けのアドレス。動いているほうが実際のアドレスをまだ書いていないときに使う。
    pub configured: SocketAddr,
}

impl AlreadyRunning {
    /// 動いているほうが書いた、実際の待ち受けのアドレス。ポートをずらして起動していることがある。
    /// 応答できるようになってから書かれるので、起動の途中なら `None`。
    pub fn addr(&self) -> Option<SocketAddr> {
        std::fs::read_to_string(&self.addr_path)
            .ok()?
            .trim()
            .parse()
            .ok()
    }
}

impl StartupError {
    fn new(message: String) -> Self {
        Self {
            message,
            already_running: None,
        }
    }
}

/// 動いているサーバー。`stop` で止める。
pub struct Running {
    /// 実際の待ち受けのアドレス。設定のポートが使えずにずらしたときは `configured` と違う。
    pub addr: SocketAddr,
    pub configured: SocketAddr,
    handle: std::thread::JoinHandle<()>,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    log_guard: WorkerGuard,
    // 保持しているあいだ二重起動を防ぐ。drop でロックが外れる。
    _instance_lock: File,
}

impl Running {
    /// サーバーに停止を伝え、DB を閉じるまで待ってから、バッファ中のログを書き出す。
    /// サーバーが既に自分で終わっていても安全に呼べる (停止の通知は無視され、join は即戻る)。
    pub fn stop(self) {
        tracing::info!("stopping server");
        let _ = self.shutdown_tx.send(());
        let _ = self.handle.join();
        drop(self.log_guard);
    }
}

/// サーバースレッドが終わったことを知らせる受け口。
/// こちらから停止を指示していないのに `wait` が返ったら、サーバーが自分で落ちたということ
/// (`axum::serve` の異常終了など)。呼び出し元 (トレイ・コンソール) は、これを検知して
/// プロセスを終わらせる。コンソールで動かしたときは 0 以外の終了コードで呼び出し元に知らせる (→ docs/distribution.md「常駐 (Windows)」)。
pub struct ServerExited(mpsc::Receiver<()>);

impl ServerExited {
    /// サーバースレッドが終わるまでブロックする。
    pub fn wait(self) {
        let _ = self.0.recv();
    }
}

/// 設定の読み込みからポートの bind までを済ませ、サーバーを動かし始める。
///
/// `folder_picker` は「公開できるフォルダー」を選ぶ窓を出す手段 (→ `crate::folder_picker`)。
pub fn start(
    on_port_unavailable: OnPortUnavailable,
    folder_picker: FolderPicker,
) -> Result<(Running, ServerExited), StartupError> {
    // ログはまだ有効化していない (出力先が設定に依存するため)。ここまでの失敗は
    // ログに残せないので、呼び出し元が伝える。
    let dirs = AppDirs::resolve().map_err(|err| StartupError::new(error_chain(&err)))?;
    let config = Config::load(&dirs).map_err(|err| StartupError::new(error_chain(&err)))?;
    let log_guard = logging::init(&config, &dirs)
        .map_err(|err| StartupError::new(format!("failed to initialize logging: {err}")))?;

    // ここから先の失敗はログにも残す。`log_guard` を drop してから返すので、書き出しは済んでいる。
    let fail = |message: String, log_guard: WorkerGuard| {
        tracing::error!("{message}");
        drop(log_guard);
        StartupError::new(message)
    };

    // `config.toml` を手で置けるよう、無ければ置き場所を作っておく (マニュアルは、メモ帳でここへ保存する手順を案内している)。
    // 作れなくてもサーバーは動くので、止めない。
    if let Err(err) = config::create_owner_only_dir(&dirs.config_dir) {
        tracing::warn!(%err, path = %dirs.config_dir.display(), "failed to create the config directory");
    }

    let addr = config.server.socket_addr();
    tracing::info!(
        version = crate::APP_VERSION,
        bind = %addr,
        config_dir = %dirs.config_dir.display(),
        log_dir = %dirs.log_dir().display(),
        "starting"
    );

    // DB へ同時アクセスさせないよう、DB 接続より前に二重起動を検知する。
    let instance_lock = match single_instance::acquire(&dirs.lock_path()) {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            tracing::info!("another instance is already running");
            drop(log_guard);
            return Err(StartupError {
                message: "another instance is already running".into(),
                already_running: Some(AlreadyRunning {
                    addr_path: dirs.addr_path(),
                    configured: addr,
                }),
            });
        }
        Err(err) => {
            return Err(fail(
                format!("failed to acquire single-instance lock: {err}"),
                log_guard,
            ));
        }
    };

    // 前に動いていたときのものが残っていれば消す。重ねて起動したほうが、古いアドレスを開かないように。
    remove_addr_file(&dirs.addr_path());

    // 設定不備 (secret の base64 長不正等) なら、DB に触れる前に失敗させる。
    let session_key = match session::resolve_key(&config.session, &dirs.session_key_path()) {
        Ok(key) => key,
        Err(err) => return Err(fail(error_chain(&err), log_guard)),
    };

    // DB の準備より先に待ち受けを決める。ずらしたポートを他の端末に案内するため (→ `lan_port`)。
    // 準備が済むまでに届いた接続は、OS の待ち行列で待つ。
    let listener = match bind(addr, on_port_unavailable) {
        Ok(listener) => listener,
        Err(err) => return Err(fail(error_chain(&err), log_guard)),
    };
    let configured = addr;
    let addr = match listener.local_addr() {
        Ok(actual) => actual,
        Err(err) => {
            return Err(fail(
                format!("failed to read the listening address: {err}"),
                log_guard,
            ));
        }
    };
    if addr != configured {
        tracing::warn!(%configured, %addr, "the configured port was unavailable; listening on another port");
    }

    let addr_path = dirs.addr_path();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let (exited_tx, exited_rx) = mpsc::channel();
    let handle = spawn(
        listener,
        session_key,
        config,
        dirs,
        folder_picker,
        Lifecycle {
            ready_tx,
            shutdown_rx,
            exited_tx,
        },
    );

    match ready_rx.recv() {
        Ok(Ok(())) => {
            // 重ねて起動したほうが、動いているほうの画面を開くのに読む (→ `AlreadyRunning`)。
            // 応答できるようになってから書くので、ファイルがあることを起動が済んだ合図にも使える
            // (待ち受け自体は DB の準備より前に始まっていて、つながるだけでは見分けられない)。
            // ロックファイルに書かないのは、Windows のロックがほかのプロセスからの読み取りも止めるため。
            if let Err(err) = std::fs::write(&addr_path, addr.to_string()) {
                tracing::warn!(%err, "failed to write the listening address");
            }
            Ok((
                Running {
                    addr,
                    configured,
                    handle,
                    shutdown_tx,
                    log_guard,
                    _instance_lock: instance_lock,
                },
                ServerExited(exited_rx),
            ))
        }
        Ok(Err(err)) => {
            let _ = handle.join();
            Err(fail(error_chain(err.as_ref()), log_guard))
        }
        Err(_) => {
            let _ = handle.join();
            Err(fail(
                "server thread exited before startup".into(),
                log_guard,
            ))
        }
    }
}

type StartupResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

/// サーバースレッドと呼び出し元のやり取り (→ `spawn`)。
struct Lifecycle {
    /// 起動が成功したか (DB接続からサーバーの組み立てまで) を呼び出し元に伝える。
    ready_tx: mpsc::Sender<StartupResult>,
    /// graceful shutdown の通知を受ける。
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    /// スレッドが終わったことを伝える (→ `ServerExited`)。
    exited_tx: mpsc::Sender<()>,
}

/// 起動失敗の本文は起動失敗の表示にそのまま出るので、OS のエラーだけでなく
/// どのアドレスで待ち受けに失敗したかを添える (使用中・予約済みのポートを利用者が直せるように)。
#[derive(Debug, thiserror::Error)]
#[error("failed to listen on {addr}")]
struct BindError {
    addr: SocketAddr,
    source: std::io::Error,
}

/// `addr` で待ち受ける。`TryNext` なら、ポートが使えないときに上の番号を順に試す。
/// すべて使えなければ、設定のポートでの失敗を返す (利用者が直すのはそちらなので)。
fn bind(
    addr: SocketAddr,
    on_port_unavailable: OnPortUnavailable,
) -> Result<TcpListener, BindError> {
    let first_err = match TcpListener::bind(addr) {
        Ok(listener) => return Ok(listener),
        Err(err) => err,
    };
    if on_port_unavailable == OnPortUnavailable::Fail || !port_unavailable(&first_err) {
        return Err(BindError {
            addr,
            source: first_err,
        });
    }
    tracing::warn!(%addr, err = %first_err, "the configured port is unavailable; trying the next ports");
    for offset in 1..=PORT_FALLBACK_ATTEMPTS {
        let Some(port) = addr.port().checked_add(offset) else {
            break;
        };
        let next = SocketAddr::new(addr.ip(), port);
        match TcpListener::bind(next) {
            Ok(listener) => return Ok(listener),
            Err(err) if port_unavailable(&err) => continue,
            Err(err) => {
                return Err(BindError {
                    addr: next,
                    source: err,
                });
            }
        }
    }
    Err(BindError {
        addr,
        source: first_err,
    })
}

/// ほかの番号なら待ち受けられる見込みのある失敗か。使用中のほか、Windows では
/// Hyper-V などが予約した範囲のポートが「アクセス拒否」(10013) になる。
fn port_unavailable(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        ErrorKind::AddrInUse | ErrorKind::PermissionDenied
    )
}

fn remove_addr_file(path: &std::path::Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => tracing::warn!(%err, "failed to remove the stale listening address"),
    }
}

/// 別スレッド・専用tokioランタイムでDB接続からHTTPサーバーの提供までを行う。
/// 呼び出し元のスレッドはタスクトレイのイベントループやシグナルの待ち受けに使うため、
/// サーバーは別スレッドに逃がす。
///
/// DB接続・マイグレーション・Router組み立てもこのランタイムの中で行う(呼び出し元で
/// 一時的なランタイムを使って済ませない)。sqlxのプール保守タスクや、下で spawn する
/// 期限切れセッション削除タスクは `tokio::spawn` した時点のランタイムに紐付き、
/// そのランタイムが破棄されると一緒に消える。サーバーの寿命と同じランタイムの中で
/// 行わないと、これらのタスクが起動直後に死んだ状態になってしまう。
///
/// 呼び出し元とのやり取りは `Lifecycle` で行う。
fn spawn(
    listener: TcpListener,
    session_key: Key,
    config: Config,
    dirs: AppDirs,
    folder_picker: FolderPicker,
    lifecycle: Lifecycle,
) -> std::thread::JoinHandle<()> {
    let Lifecycle {
        ready_tx,
        shutdown_rx,
        exited_tx,
    } = lifecycle;
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("failed to build tokio runtime");
        rt.block_on(async move {
            let addr = match listener.local_addr() {
                Ok(addr) => addr,
                Err(err) => {
                    let _ = ready_tx.send(Err(err.into()));
                    return;
                }
            };
            let pool = match db::connect(&dirs.db_path()).await {
                Ok(pool) => pool,
                Err(err) => {
                    let _ = ready_tx.send(Err(err.into()));
                    return;
                }
            };
            // 他の端末に mDNS の名前で案内するポート (→ docs/architecture.md「LAN からの到達性」)。
            // OS の mDNS はホストの全インターフェースのアドレスで応えるので、全インターフェースで
            // 待ち受けている (`bind = "0.0.0.0"`) ときだけ案内する。それ以外の bind では、名前が指す
            // アドレスの一部でしかサーバーが待ち受けておらず、他端末には開けない URL を案内しうる。
            // `IpAddr::is_unspecified()` は IPv6 の `::` でも true になるが、`bind` は
            // `IpAddr::V4(Ipv4Addr::UNSPECIFIED)` の1点だけを既定・サポート対象としている
            // (config.example.toml 参照。IPv6 bind は未サポート) ので、それと厳密に比較する。
            let lan_port = (addr.ip() == IpAddr::V4(Ipv4Addr::UNSPECIFIED)).then_some(addr.port());

            // 管理画面に今のポートとして出す値。ずらして起動したときは実際の番号にする。
            let mut config = config;
            config.server.port = addr.port();

            // ここから先で起動に失敗して返るときは、必ずプールを閉じてから返る。開いたままだと、
            // Windows では DB ファイルを消せない (呼び出し元が片付けられない)。
            let setup: Result<_, Box<dyn std::error::Error + Send + Sync>> = async {
                db::migrate(&pool).await?;
                crate::folder_access::restore(&pool).await;
                build_app(
                    pool.clone(),
                    session_key,
                    &config,
                    &dirs,
                    lan_port,
                    folder_picker,
                )
                .await
            }
            .await;

            let app = match setup {
                Ok(app) => app,
                Err(err) => {
                    pool.close().await;
                    let _ = ready_tx.send(Err(err));
                    return;
                }
            };

            // bind は呼び出し元で済ませてある。tokio へ渡すには非ブロッキングにする。
            let listener = match listener
                .set_nonblocking(true)
                .and_then(|()| tokio::net::TcpListener::from_std(listener))
            {
                Ok(listener) => listener,
                Err(err) => {
                    pool.close().await;
                    let _ = ready_tx.send(Err(Box::new(BindError { addr, source: err })));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(()));
            tracing::info!(%addr, "server started");

            // 期限切れセッションの定期削除(1分毎)。`SqliteStore`は期限切れの行を
            // 自分では消さないため、削除タスクをこちらで回す。
            let session_store = SqliteStore::new(pool.clone());
            // `continuously_delete_expired`はDBエラーでループを終了しうる(Result<()>を
            // 返す)。JoinHandleを握り潰すと、以後ずっと削除が止まっていても気づけない
            // ため、ここでawaitしてログに残す。
            tokio::spawn(async move {
                if let Err(err) = session_store
                    .continuously_delete_expired(Duration::from_secs(60))
                    .await
                {
                    tracing::error!(%err, "expired session cleanup task stopped");
                }
            });

            // 停止を指示されたら、処理中の応答が終わるのを SHUTDOWN_GRACE まで待つ。
            // 動画の再生などで接続が続くと、いつまでも止まらないため。
            let (stopping_tx, stopping_rx) = tokio::sync::oneshot::channel::<()>();
            // 接続元のアドレスをハンドラに渡す (`ConnectInfo`)。初回セットアップの口が
            // ループバックからの要求だけを通すのに要る (→ `api::setup`)。
            let serve = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
                let _ = stopping_tx.send(());
            });
            tokio::select! {
                result = serve => {
                    if let Err(err) = result {
                        tracing::error!(%err, "server exited with an error");
                    }
                }
                _ = async {
                    if stopping_rx.await.is_ok() {
                        tokio::time::sleep(SHUTDOWN_GRACE).await;
                    } else {
                        std::future::pending::<()>().await;
                    }
                } => {
                    tracing::warn!("connections did not close in time; stopping anyway");
                }
            }

            // 打ち切った接続のタスクがまだ DB 接続を握っていると、close は返ってこない。
            if tokio::time::timeout(POOL_CLOSE_GRACE, pool.close())
                .await
                .is_err()
            {
                tracing::warn!("database connections did not close in time");
            }
            tracing::info!("server stopped");
        });
        // 打ち切った接続のタスクが残っていても待たない (ランタイムの drop は待ち続けうる)。
        rt.shutdown_timeout(RUNTIME_SHUTDOWN_GRACE);
        // block_on を抜けた = サーバーが終わった。停止指示によるものでも、異常終了でも
        // ここに来る。停止指示を出したかどうかは呼び出し元 (`ServerExited`) が知っている。
        let _ = exited_tx.send(());
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TempDir, unused_loopback_addr};

    /// `handle.join()`が想定通り終わることを確認する。`spawn`の実装が
    /// graceful shutdownを正しく処理できていない場合、テストが無期限にハングする
    /// 代わりに明確に失敗させたい。
    fn join_with_timeout(handle: std::thread::JoinHandle<()>, timeout: Duration) {
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = handle.join();
            let _ = done_tx.send(());
        });
        done_rx
            .recv_timeout(timeout)
            .expect("server thread did not shut down in time");
    }

    #[test]
    fn spawn_reports_error_via_ready_channel_on_db_connect_failure() {
        let tmp = TempDir::new("connect-failure");
        // 親ディレクトリになるべき場所が既に(ディレクトリではなく)ファイルだと、
        // create_dir_all が失敗する。DB接続失敗を再現するための手軽な方法。
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, b"").unwrap();

        let (ready_tx, ready_rx) = mpsc::channel();
        let (exited_tx, _exited_rx) = mpsc::channel();
        let (_shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let addr = unused_loopback_addr();

        let handle = spawn(
            TcpListener::bind(addr).expect("空いているはずのポートで待ち受けられなかった"),
            Key::generate(),
            Config::default(),
            AppDirs {
                config_dir: tmp.path().to_path_buf(),
                data_dir: blocker,
            },
            FolderPicker::new(|| Ok(None)),
            Lifecycle {
                ready_tx,
                shutdown_rx,
                exited_tx,
            },
        );

        let result = ready_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("ready_txから応答が来なかった");
        assert!(result.is_err(), "DB接続失敗のはずがOkだった");

        join_with_timeout(handle, Duration::from_secs(10));
    }

    /// `Fail` では、設定のポートが使えなければずらさずに失敗する。
    #[test]
    fn bind_fails_on_occupied_port_without_fallback() {
        let addr = unused_loopback_addr();
        let _occupied = TcpListener::bind(addr).expect("ふさぐためのポートを取れなかった");

        let err = bind(addr, OnPortUnavailable::Fail).expect_err("使用中のポートで待ち受けられた");
        // 起動失敗の本文はメッセージボックスにそのまま出る。どのポートかが分からないと利用者が直せない。
        let message = error_chain(&err);
        assert!(
            message.contains(&addr.to_string()),
            "本文に待ち受けのアドレスが無い: {message}"
        );
        // 包んだあとも OS のエラーを source として残す (マニュアルは `os error 10048` を手がかりに案内している)。
        let source = std::error::Error::source(&err).expect("OS のエラーが source に無い");
        assert!(
            message.contains(&source.to_string()),
            "本文に OS のエラーが無い: {message}"
        );
    }

    /// `TryNext` では、設定のポートが使えなければ上の番号で待ち受ける。
    #[test]
    fn bind_moves_to_a_higher_port_when_occupied() {
        let addr = unused_loopback_addr();
        let _occupied = TcpListener::bind(addr).expect("ふさぐためのポートを取れなかった");

        let listener =
            bind(addr, OnPortUnavailable::TryNext).expect("ずらして待ち受けられなかった");
        let actual = listener
            .local_addr()
            .expect("待ち受けのアドレスを読めなかった");
        assert_eq!(actual.ip(), addr.ip());
        assert!(
            actual.port() > addr.port() && actual.port() <= addr.port() + PORT_FALLBACK_ATTEMPTS,
            "ずらした先が範囲の外: {actual}"
        );
    }

    #[test]
    fn already_running_reads_the_written_address() {
        let tmp = TempDir::new("already-running-addr");
        let running = AlreadyRunning {
            addr_path: tmp.path().join("weblav.addr"),
            configured: "0.0.0.0:3000"
                .parse()
                .expect("アドレスの書き方が正しくない"),
        };
        assert_eq!(running.addr(), None, "書かれる前は None のはず");

        std::fs::write(&running.addr_path, "0.0.0.0:3001").expect("weblav.addr を書けなかった");
        assert_eq!(
            running.addr(),
            Some(
                "0.0.0.0:3001"
                    .parse()
                    .expect("アドレスの書き方が正しくない")
            )
        );
    }

    #[test]
    fn spawn_reports_ready_then_shuts_down_gracefully_on_signal() {
        let tmp = TempDir::new("graceful-shutdown");

        let (ready_tx, ready_rx) = mpsc::channel();
        let (exited_tx, _exited_rx) = mpsc::channel();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let addr = unused_loopback_addr();

        let handle = spawn(
            TcpListener::bind(addr).expect("空いているはずのポートで待ち受けられなかった"),
            Key::generate(),
            Config::default(),
            AppDirs {
                config_dir: tmp.path().to_path_buf(),
                data_dir: tmp.path().to_path_buf(),
            },
            FolderPicker::new(|| Ok(None)),
            Lifecycle {
                ready_tx,
                shutdown_rx,
                exited_tx,
            },
        );

        let result = ready_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("ready_txから応答が来なかった");
        assert!(result.is_ok(), "起動に成功したはずがErrだった: {result:?}");

        // シャットダウン通知後、pool.close()まで含めてスレッドが終了することを確認する。
        // ここでハングするなら with_graceful_shutdown の配線が壊れている。
        shutdown_tx.send(()).unwrap();
        join_with_timeout(handle, Duration::from_secs(10));
    }
}
