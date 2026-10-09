//! 初回セットアップ (→ docs/access.md「初回セットアップ」) の、タスクトレイ側の問い合わせ。
//! どちらの口もループバックからしか通らないので、問い合わせ先は `localhost` に限る。

use std::net::SocketAddr;
use std::time::Duration;

use super::browser_origin;
use super::http::{self, Response};

/// 管理者がまだいないか (メニューに「セットアップ」を出すか)。
/// 問い合わせられないときは出さない。
pub fn required(addr: SocketAddr, timeout: Duration) -> bool {
    matches!(
        http::request(addr, "GET", "/api/v1/setup/status", timeout),
        Some(Response { status: 200, .. })
    )
}

/// ブラウザーで開くセットアップ画面の URL を組み立てる。
///
/// トークンを受け取れなかったときも、トークン無しの URL を返して開かせる
/// (画面が「使えないリンク」だと伝える)。何も起きないと、押せたのかが分からないため。
/// トークンは16進の文字列なので、URL に載せるときの変換は要らない。
pub fn url(addr: SocketAddr, timeout: Duration) -> String {
    let origin = browser_origin(addr);
    match http::request(addr, "POST", "/api/v1/setup/token", timeout) {
        Some(Response { status: 200, body }) if is_token(&body) => {
            format!("{origin}/setup?token={body}")
        }
        _ => format!("{origin}/setup"),
    }
}

/// 発行されるトークンの形 (→ `api::setup`)。同じポートで別のものが待ち受けていたときに、
/// 応答をそのまま URL へ入れないための確認。
fn is_token(body: &str) -> bool {
    body.len() == 64 && body.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::unused_loopback_addr;
    use crate::tray::http::serve_once;

    const TIMEOUT: Duration = Duration::from_secs(5);

    #[test]
    fn setup_is_required_only_when_the_server_says_200() {
        let addr = serve_once("HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n");
        assert!(required(addr, TIMEOUT));
    }

    #[test]
    fn setup_is_not_required_when_the_server_says_404() {
        let addr = serve_once("HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n");
        assert!(!required(addr, TIMEOUT));
    }

    /// サーバーが止まっている間は、押せる項目を増やさない。
    #[test]
    fn setup_is_not_required_when_nothing_listens() {
        assert!(!required(unused_loopback_addr(), TIMEOUT));
    }

    /// 発行されるトークンと同じ形 (16進64桁)。
    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn url_carries_the_issued_token() {
        let addr = serve_once(concat!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\n\r\n",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert_eq!(
            url(addr, TIMEOUT),
            format!("http://localhost:{}/setup?token={TOKEN}", addr.port())
        );
    }

    /// 別のものが待ち受けていて、トークンに見えない本文が返ったとき。
    #[test]
    fn url_ignores_a_body_that_is_not_a_token() {
        let addr = serve_once("HTTP/1.1 200 OK\r\ncontent-type: text/html\r\n\r\n<html>hello");
        assert_eq!(
            url(addr, TIMEOUT),
            format!("http://localhost:{}/setup", addr.port())
        );
    }

    #[test]
    fn url_falls_back_to_the_page_without_a_token() {
        let addr = serve_once("HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n");
        assert_eq!(
            url(addr, TIMEOUT),
            format!("http://localhost:{}/setup", addr.port())
        );
    }
}
