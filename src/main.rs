// `debug_assertions`で分けているのは、`cargo run`・`just dev-backend`では
// 素のコンソール出力を残したいため (→ docs/distribution.md「コンソールの扱い」)。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod tray;
// テスト用の共有ヘルパー (lib クレートと同じファイルを読む)。このクレートで使わない
// ものも含むので、モジュールごと dead_code を許可する。
#[cfg(test)]
#[path = "test_support.rs"]
#[allow(dead_code)]
mod test_support;

fn main() {
    // 本体はタスクトレイ常駐専用で、引数を受け付けない。開発用の CLI は `weblav-cli`
    // (→ weblav::cli)。引数付きで呼ばれたら、黙って常駐せずに伝えて終える。
    if std::env::args_os().len() > 1 {
        fatal_startup_error("weblav takes no command-line options");
    }

    // サーバーを同じプロセスで動かしてから、トレイを出す (→ docs/distribution.md「常駐 (Windows)」)。
    // 設定のポートが使えなければずらして起動し、トレイで知らせる (→ docs/distribution.md「常駐 (Windows)」)。
    // フォルダ選択の窓はメインスレッドでしか開けない OS があるので、トレイのイベントループで開く。
    let (folder_picker, pick_requests) = weblav::folder_picker::on_main_thread();
    match weblav::server::start(weblav::server::OnPortUnavailable::TryNext, folder_picker) {
        Ok((server, exited)) => tray::run(server, exited, pick_requests),
        // スタートアップで起動したあとに、スタートメニューから開いたときなど。
        // ウィンドウを持たないので、黙って終えると起動しなかったように見える。代わりに画面を開く。
        Err(weblav::server::StartupError {
            already_running: Some(running),
            ..
        }) => tray::open_in_browser(&running),
        Err(err) => fatal_startup_error(&err.message),
    }
}

/// タスクトレイを出す前の起動失敗を報告して終了する。
/// ダブルクリック起動ではコンソールが無いため、stderrに加えてメッセージボックスでも伝える
/// (→ docs/distribution.md「コンソールの扱い」)。
fn fatal_startup_error(message: &str) -> ! {
    // `eprintln!`は書き込み失敗でパニックする(GUIサブシステムでstderrが無効な場合等)。
    // ここでパニックすると後続のメッセージボックスに辿り着けなくなるため、
    // 失敗しても無視して続ける書き方にする。
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "{message}");
    #[cfg(windows)]
    show_message_box(message);
    std::process::exit(1);
}

#[cfg(windows)]
fn show_message_box(message: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    use windows::core::HSTRING;

    let text = HSTRING::from(message);
    let caption = HSTRING::from("WebLAV");
    unsafe {
        MessageBoxW(None, &text, &caption, MB_OK | MB_ICONERROR);
    }
}
