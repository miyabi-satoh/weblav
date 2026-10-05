//! サーバーだけを動かす exe。画面を持たない。開発 (`just dev-backend`・e2e) と、Linux で systemd などから動かすのに使う。
//! Ctrl+C (Unix では SIGTERM も) を受けるか、サーバーが自分で落ちるまで動く (→ docs/distribution.md「常駐 (Windows)」)。

use std::process::ExitCode;
use std::sync::mpsc;

fn main() -> ExitCode {
    if std::env::args_os().len() > 1 {
        eprintln!("weblav-service takes no arguments");
        return ExitCode::FAILURE;
    }

    run_in_foreground()
}

/// メインスレッドを起こす理由。停止シグナルによる停止は正常、サーバーの異常終了は失敗として扱う。
enum Wake {
    Signal,
    ServerExited,
    /// フォルダ選択の窓を開く頼み。macOS ではメインスレッドでしか開けない (→ weblav::folder_picker)。
    PickFolder(weblav::folder_picker::PickRequest),
}

fn run_in_foreground() -> ExitCode {
    let (folder_picker, pick_requests) = weblav::folder_picker::on_main_thread();
    let (server, exited) =
        match weblav::server::start(weblav::server::OnPortUnavailable::Fail, folder_picker) {
            Ok(running) => running,
            Err(err) => {
                eprintln!("{}", err.message);
                return ExitCode::FAILURE;
            }
        };

    // 停止シグナル・サーバーの異常終了・フォルダ選択の窓を開く頼みを、1つのチャネルで待つ。
    let (wake_tx, wake_rx) = mpsc::channel();
    let pick_tx = wake_tx.clone();
    pick_requests.forward(move |request| pick_tx.send(Wake::PickFolder(request)).is_ok());
    let signal_tx = wake_tx.clone();
    std::thread::spawn(move || {
        wait_for_shutdown_signal();
        let _ = signal_tx.send(Wake::Signal);
    });
    std::thread::spawn(move || {
        exited.wait();
        let _ = wake_tx.send(Wake::ServerExited);
    });

    let stopped_by_signal = loop {
        match wake_rx.recv().unwrap_or(Wake::ServerExited) {
            // macOS の窓はメインスレッドでしか開けない。ほかの OS では別のスレッドで開き、
            // 窓を開いている間も停止シグナルを受けられるようにする。
            Wake::PickFolder(request) => {
                if cfg!(target_os = "macos") {
                    request.serve();
                } else {
                    std::thread::spawn(move || request.serve());
                }
            }
            Wake::Signal => break true,
            Wake::ServerExited => break false,
        }
    };
    server.stop();
    if stopped_by_signal {
        ExitCode::SUCCESS
    } else {
        // サーバーが自分で落ちた。0 以外で終わり、systemd などの再起動に委ねる。
        eprintln!("the server stopped unexpectedly");
        ExitCode::FAILURE
    }
}

/// Ctrl+C か、Unix では SIGTERM を待つ。systemd はサービスを止めるときに SIGTERM を送る。
/// tokio の signal はランタイムを要するので、待つあいだだけ軽量な current-thread ランタイムを持つ。
fn wait_for_shutdown_signal() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    rt.block_on(async {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let mut terminate =
                signal(SignalKind::terminate()).expect("failed to listen for SIGTERM");
            tokio::select! {
                result = tokio::signal::ctrl_c() => result.expect("failed to listen for Ctrl+C"),
                _ = terminate.recv() => {}
            }
        }
        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to listen for Ctrl+C");
        }
    });
    tracing::info!("shutdown signal received");
}
