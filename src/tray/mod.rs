//! タスクトレイ。サーバーを同じプロセスで動かす (→ docs/distribution.md「常駐 (Windows)」)。

mod embedded;
mod http;
mod login_item;
mod setup;

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};

use tray_icon::menu::Menu;
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use weblav::server::AlreadyRunning;

pub use embedded::run;

#[cfg(windows)]
fn detach_console() {
    use windows::Win32::System::Console::FreeConsole;
    unsafe {
        let _ = FreeConsole();
    }
}

#[cfg(not(windows))]
fn detach_console() {}

/// セットアップが要るかをサーバーへ問い合わせる間隔。
const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// 1回の問い合わせを諦めるまでの時間。自分の PC の中への問い合わせなので、応答が
/// これより遅ければ動いていないのと変わらない。
const POLL_TIMEOUT: Duration = Duration::from_secs(2);

/// 二重起動したときに、動いているほうが応答できるようになるのを待つ上限。
const STARTUP_WAIT: Duration = Duration::from_secs(15);
/// 応答できるようになったかを確かめ直す間隔。
const STARTUP_RETRY_INTERVAL: Duration = Duration::from_millis(500);

/// タスクトレイのメニューの表示言語 (→ docs/architecture.md「タスクトレイの表示言語」)。ツールチップは言語で変えない。
/// フロントエンドの言語トグルとは独立の、日本語/英語の二択。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayLocale {
    Ja,
    En,
}

impl TrayLocale {
    fn open_label(self) -> &'static str {
        match self {
            TrayLocale::Ja => "ブラウザで開く",
            TrayLocale::En => "Open in browser",
        }
    }

    /// 別の画面 (ブラウザのマニュアル) へ移る項目なので体言止めにする (→ docs/ui.md「UI 全般」)。
    fn manual_label(self) -> &'static str {
        match self {
            TrayLocale::Ja => "マニュアル",
            TrayLocale::En => "Manual",
        }
    }

    /// 別の画面 (ブラウザのセットアップ画面) へ移る項目なので体言止めにする (→ docs/ui.md「UI 全般」)。
    fn setup_label(self) -> &'static str {
        match self {
            TrayLocale::Ja => "セットアップ",
            TrayLocale::En => "Setup",
        }
    }

    /// 設定のポートが使えず、ずらして起動したことを知らせる項目 (押せない)。
    fn port_moved_label(self, configured: u16, actual: u16) -> String {
        match self {
            TrayLocale::Ja => {
                format!("ポート {configured} を使えないため、{actual} で動いています")
            }
            TrayLocale::En => format!("Port {configured} was unavailable; running on {actual}"),
        }
    }

    /// 「ログイン時に起動」。Windows では、OS やマニュアルと同じく「サインイン」と書く。
    /// アプリから変えられない状態のときは、どこで変えるかを添える。
    fn login_item_label(self, state: login_item::State) -> &'static str {
        use login_item::State;
        match (self, state, cfg!(windows)) {
            (TrayLocale::Ja, State::OffLocked, _) => {
                "サインイン時に起動 (Windows の設定で止められています)"
            }
            (TrayLocale::Ja, State::NeedsApproval, _) => {
                "ログイン時に起動 (システム設定で許可してください)"
            }
            (TrayLocale::Ja, _, true) => "サインイン時に起動",
            (TrayLocale::Ja, _, false) => "ログイン時に起動",
            (TrayLocale::En, State::OffLocked, _) => {
                "Start at sign-in (turned off in Windows Settings)"
            }
            (TrayLocale::En, State::NeedsApproval, _) => {
                "Start at login (allow it in System Settings)"
            }
            (TrayLocale::En, _, true) => "Start at sign-in",
            (TrayLocale::En, _, false) => "Start at login",
        }
    }

    fn quit_label(self) -> &'static str {
        match self {
            TrayLocale::Ja => "終了する",
            TrayLocale::En => "Quit",
        }
    }
}

/// Win32 の `LANGID` (`GetUserDefaultUILanguage` の戻り値) から表示言語を判定する。
/// OS呼び出しから切り離してあるのは、実際のOSの言語設定に依らずテストするため。
///
/// 下位10bit (`PRIMARYLANGID`) だけを見る。上位6bit の `SUBLANGID`
/// (地域: 日本語はほぼ`SUBLANG_JAPANESE_JAPAN`のみだが判定に含める理由がない) は無視する。
// Windows 以外では `tray_locale` から呼ばれず、テストからしか使わない。
#[cfg_attr(not(any(windows, test)), expect(dead_code))]
fn locale_from_langid(langid: u16) -> TrayLocale {
    /// `LANG_JAPANESE` (winnt.h)。この定数のためだけに windows-sys を足さない。
    const LANG_JAPANESE: u16 = 0x11;
    const PRIMARY_LANGID_MASK: u16 = 0x3ff;

    if langid & PRIMARY_LANGID_MASK == LANG_JAPANESE {
        TrayLocale::Ja
    } else {
        TrayLocale::En
    }
}

/// OS の表示言語を判定する。
#[cfg(windows)]
fn tray_locale() -> TrayLocale {
    use windows::Win32::Globalization::GetUserDefaultUILanguage;
    locale_from_langid(unsafe { GetUserDefaultUILanguage() })
}

/// macOS は、OS の優先する言語の一覧の先頭を見る。ログイン項目や Finder から起動すると
/// `LANG` が渡らないため、環境変数では決められない。
#[cfg(target_os = "macos")]
fn tray_locale() -> TrayLocale {
    let first = objc2_foundation::NSLocale::preferredLanguages().firstObject();
    first.map_or(TrayLocale::En, |tag| locale_from_name(&tag.to_string()))
}

/// Windows・macOS 以外 (Linux) は、POSIX のロケール環境変数から判定する。
/// 優先順位 (`LC_ALL` → `LC_MESSAGES` → `LANG`) も POSIX に倣う。
#[cfg(not(any(windows, target_os = "macos")))]
fn tray_locale() -> TrayLocale {
    locale_from_env(|name| std::env::var(name).ok())
}

/// `locale_from_env` の値取得を差し替え可能にして、実際の環境変数に依らずテストする。
// Windows・macOS では `tray_locale` から呼ばれず、テストからしか使わない。
#[cfg_attr(all(any(windows, target_os = "macos"), not(test)), expect(dead_code))]
fn locale_from_env(var: impl Fn(&str) -> Option<String>) -> TrayLocale {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|name| var(name).filter(|v| !v.is_empty()))
        .map_or(TrayLocale::En, |v| locale_from_name(&v))
}

/// ロケール名 (`ja_JP.UTF-8`) や言語タグ (`ja-JP`) から判定する。どちらも言語部分が先頭に来る。
/// 言語部分を区切りまで取り出して比べるのは、`jam` のような ja で始まる別の言語と分けるため。
fn locale_from_name(name: &str) -> TrayLocale {
    let language = name.split(['-', '_', '.', '@']).next().unwrap_or_default();
    if language.eq_ignore_ascii_case("ja") {
        TrayLocale::Ja
    } else {
        TrayLocale::En
    }
}

/// SVG から生成した生のRGBAバッファ (→ scripts/generate-icon.mjs)。
/// 実際の表示は16〜32px程度だが、高DPI環境での縮小表示をきれいにするため
/// 大きめに持つ (→ docs/distribution.md、実機確認: 100%/150%)。
/// パースが要らない生データにしてあるのは、実行時に画像デコード用のcrateを
/// 増やさないため。
/// 生成元は `assets/icon.svg`。
#[cfg(not(target_os = "macos"))]
const TRAY_ICON_RGBA: &[u8] = include_bytes!("../../assets/tray-icon-64.rgba");
/// macOS のメニューバーは単色のテンプレート画像が作法 (色はOSが明暗に合わせて付ける)。
/// `assets/tray-icon-mac.svg` から生成する。
#[cfg(target_os = "macos")]
const TRAY_ICON_RGBA: &[u8] = include_bytes!("../../assets/tray-icon-mac-64.rgba");

/// 一辺の長さ (px)。正方形のRGBA (4バイト/px) のはずなので、バイト数から逆算する。
/// 生成スクリプト側でサイズを変えても、ここで数値を直し忘れる心配が無い。
/// 空、または正方形のRGBAとして辻褄が合わないバイト数なら `None`。
fn tray_icon_side(rgba_len: usize) -> Option<u32> {
    let side = (rgba_len / 4).isqrt();
    // 4の倍数でない長さも、ここで逆算した値と辻褄が合わず弾かれる。
    if side == 0 || side * side * 4 != rgba_len {
        return None;
    }
    u32::try_from(side).ok()
}

fn make_tray_icon() -> Icon {
    let side =
        tray_icon_side(TRAY_ICON_RGBA.len()).expect("tray icon asset is not a square RGBA buffer");
    Icon::from_rgba(TRAY_ICON_RGBA.to_vec(), side, side).expect("failed to build tray icon")
}

/// 組み立てたメニューとツールチップからトレイアイコンを作る。
/// tray-icon はメインスレッドかつイベントループ稼働中に作る必要があるため、
/// `NewEvents(Init)` を受けてから呼ぶこと。
fn build_tray(menu: Menu, tooltip: &str) -> TrayIcon {
    let builder = TrayIconBuilder::new();
    // macOS のメニューバーでは、明暗に合わせて OS が塗り直すテンプレートの絵として渡す。
    #[cfg(target_os = "macos")]
    let builder = builder.with_icon_templated(make_tray_icon());
    #[cfg(not(target_os = "macos"))]
    let builder = builder.with_icon(make_tray_icon());
    builder
        .with_menu(Box::new(menu))
        .with_tooltip(tooltip)
        .build()
        .expect("failed to build tray icon")
}

/// ブラウザで開くURL用のアドレスを返す。
/// `bind = 0.0.0.0` 等の未指定アドレスをそのまま使うとブラウザで開けないため、
/// その場合はループバックアドレスに読み替える。
fn open_addr(addr: SocketAddr) -> SocketAddr {
    if addr.ip().is_unspecified() {
        SocketAddr::new(
            if addr.is_ipv6() {
                Ipv6Addr::LOCALHOST.into()
            } else {
                Ipv4Addr::LOCALHOST.into()
            },
            addr.port(),
        )
    } else {
        addr
    }
}

/// ブラウザで開くときのオリジン (`http://localhost:3000` など)。
///
/// ループバックは `127.0.0.1` ではなく `localhost` と書く。`session.secure_cookie` を有効にすると、
/// Edge・Chrome は HTTP の `127.0.0.1` ではログインの Cookie を受け取らず、`localhost` だけを
/// 例外にするため (→ docs/architecture.md「LAN からの到達性」)。`127.0.0.2` のようなほかのループバックは、
/// `localhost` では届かないのでそのまま書く。
fn browser_origin(addr: SocketAddr) -> String {
    let addr = open_addr(addr);
    let ip = addr.ip();
    if ip == Ipv4Addr::LOCALHOST || ip == Ipv6Addr::LOCALHOST {
        format!("http://localhost:{}", addr.port())
    } else {
        format!("http://{addr}")
    }
}

/// 既に動いているサーバーの画面を、ブラウザで開く。管理者がまだいなければセットアップの画面を開く
/// (トレイの「セットアップ」と同じ)。
pub fn open_in_browser(running: &AlreadyRunning) {
    // 動いているほうがまだ起動の途中 (初回のマイグレーションなど) なら、応答できるようになるまで少し待つ。
    // 待たずに問い合わせると「セットアップは要らない」と取り違え、つながらない画面を開いてしまう。
    // 実際のアドレスは応答できるようになってから書かれるので、それが現れるのを待つ。
    let deadline = Instant::now() + STARTUP_WAIT;
    let addr = loop {
        if let Some(addr) = running.addr() {
            break addr;
        }
        if Instant::now() >= deadline {
            break running.configured;
        }
        std::thread::sleep(STARTUP_RETRY_INTERVAL);
    };
    let addr = open_addr(addr);
    let url = if setup::required(addr, POLL_TIMEOUT) {
        setup::url(addr, POLL_TIMEOUT)
    } else {
        browser_origin(addr)
    };
    let _ = open::that_detached(url);
}

/// タスクトレイのツールチップの内容。アドレスは出さない (→ docs/architecture.md「LAN からの到達性」)。
/// 言語でも変わらない (→ docs/architecture.md「タスクトレイの表示言語」)。
const TRAY_TOOLTIP: &str = "weblav";

#[cfg(test)]
mod tests {
    use super::*;

    /// 日本語の`LANGID`はプライマリ言語部分(下位10bit)だけで判定する。
    /// 地域(サブ言語)が変わっても日本語のままであることを含めて確認する。
    #[test]
    fn locale_from_langid_recognizes_japanese() {
        assert_eq!(locale_from_langid(0x0411), TrayLocale::Ja); // ja-JP
        // SUBLANGID が既定 (0) でもプライマリ言語だけで日本語と判定される。
        assert_eq!(locale_from_langid(0x0011), TrayLocale::Ja);
    }

    #[test]
    fn locale_from_langid_falls_back_to_english_for_others() {
        assert_eq!(locale_from_langid(0x0409), TrayLocale::En); // en-US
        assert_eq!(locale_from_langid(0x0809), TrayLocale::En); // en-GB
        assert_eq!(locale_from_langid(0x0407), TrayLocale::En); // de-DE
        assert_eq!(locale_from_langid(0x0000), TrayLocale::En); // LANG_NEUTRAL
    }

    fn env(values: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let values: Vec<(String, String)> = values
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| {
            values
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    }

    #[test]
    fn locale_from_env_recognizes_japanese() {
        assert_eq!(
            locale_from_env(env(&[("LANG", "ja_JP.UTF-8")])),
            TrayLocale::Ja
        );
        // 地域(サブタグ)が変わっても言語部分だけで判定する。
        assert_eq!(locale_from_env(env(&[("LANG", "ja")])), TrayLocale::Ja);
    }

    #[test]
    fn locale_from_env_falls_back_to_english_for_others() {
        assert_eq!(
            locale_from_env(env(&[("LANG", "en_US.UTF-8")])),
            TrayLocale::En
        );
        // 環境変数が1つも無い (GUI起動で未設定など) 場合も英語に倒す。
        assert_eq!(locale_from_env(env(&[])), TrayLocale::En);
        // 空文字は未設定と同じ扱いにする。
        assert_eq!(locale_from_env(env(&[("LANG", "")])), TrayLocale::En);
    }

    /// macOS の優先する言語は `ja-JP` のような言語タグで返る。
    #[test]
    fn locale_from_name_accepts_language_tags() {
        assert_eq!(locale_from_name("ja-JP"), TrayLocale::Ja);
        assert_eq!(locale_from_name("ja"), TrayLocale::Ja);
        assert_eq!(locale_from_name("en-JP"), TrayLocale::En);
        assert_eq!(locale_from_name("jam"), TrayLocale::En);
    }

    /// POSIX の優先順位 (`LC_ALL` → `LC_MESSAGES` → `LANG`) どおりに、
    /// より優先度の高い変数がある場合はそちらを使う。
    #[test]
    fn locale_from_env_prefers_lc_all_over_lang() {
        assert_eq!(
            locale_from_env(env(&[("LANG", "ja_JP.UTF-8"), ("LC_ALL", "en_US.UTF-8")])),
            TrayLocale::En
        );
        assert_eq!(
            locale_from_env(env(&[
                ("LANG", "en_US.UTF-8"),
                ("LC_MESSAGES", "ja_JP.UTF-8")
            ])),
            TrayLocale::Ja
        );
    }

    /// このOSで埋め込む `TRAY_ICON_RGBA` が正方形のRGBA (4バイト/px) として矛盾しない
    /// バイト数であることを確認する。矛盾すると`make_tray_icon`が起動時にパニックする。
    /// この不一致を実際に起動するより前、テストの時点で検知する。
    #[test]
    fn tray_icon_asset_is_a_valid_square_rgba_buffer() {
        assert!(tray_icon_side(TRAY_ICON_RGBA.len()).is_some());
    }

    #[test]
    fn tray_icon_side_rejects_lengths_that_are_not_square_rgba() {
        assert_eq!(tray_icon_side(64 * 64 * 4), Some(64));
        assert_eq!(tray_icon_side(0), None);
        assert_eq!(tray_icon_side(64 * 64 * 4 + 4), None); // 正方形でない
        assert_eq!(tray_icon_side(64 * 64 * 4 + 1), None); // 4の倍数でない
    }

    #[test]
    fn unspecified_addr_becomes_loopback() {
        let addr: SocketAddr = "0.0.0.0:3000".parse().unwrap();
        assert_eq!(open_addr(addr), "127.0.0.1:3000".parse().unwrap());
    }

    #[test]
    fn loopback_addr_is_unchanged() {
        let addr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
        assert_eq!(open_addr(addr), addr);
    }

    #[test]
    fn browser_origin_names_the_loopback_localhost() {
        let unspecified: SocketAddr = "0.0.0.0:3000".parse().expect("アドレスを解釈できなかった");
        assert_eq!(browser_origin(unspecified), "http://localhost:3000");
        let loopback: SocketAddr = "127.0.0.1:3100"
            .parse()
            .expect("アドレスを解釈できなかった");
        assert_eq!(browser_origin(loopback), "http://localhost:3100");
        let v6: SocketAddr = "[::1]:3000".parse().expect("アドレスを解釈できなかった");
        assert_eq!(browser_origin(v6), "http://localhost:3000");
        // `localhost` では届かないループバック・LAN のアドレスはそのまま。
        let other_loopback: SocketAddr = "127.0.0.2:3000"
            .parse()
            .expect("アドレスを解釈できなかった");
        assert_eq!(browser_origin(other_loopback), "http://127.0.0.2:3000");
        let lan: SocketAddr = "192.168.1.5:3000"
            .parse()
            .expect("アドレスを解釈できなかった");
        assert_eq!(browser_origin(lan), "http://192.168.1.5:3000");
    }
}
