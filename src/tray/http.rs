//! サーバーへの問い合わせ。
//!
//! ADR: HTTP クライアントの crate を足さず、std の TCP で1往復する。問い合わせるのは
//! 自分のサーバーの決まった数本だけで、状態行と短い本文しか読まないため。

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// 読み込む応答全体の上限。問い合わせ先はどれも状態行と短い本文しか返さない。
const RESPONSE_LIMIT: u64 = 1024;

/// HTTP の応答。
#[derive(Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

/// `addr` に1回だけ問い合わせる。`method` は `GET` か `POST` で、本文は送らない。
/// 接続できない・時間切れ・HTTP 以外の応答は、どれも `None` にする。呼び出し元はどれも
/// 200 の応答かどうかだけを見るため。
pub fn request(addr: SocketAddr, method: &str, path: &str, timeout: Duration) -> Option<Response> {
    let mut stream = TcpStream::connect_timeout(&addr, timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;

    // 本文を送らないことを明示する。省くと、本文を待たれて時間切れになりうる。
    let content_length = if method == "GET" {
        String::new()
    } else {
        "Content-Length: 0\r\n".to_string()
    };
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n{content_length}\r\n"
    );
    stream.write_all(request.as_bytes()).ok()?;

    let mut raw = Vec::new();
    stream.take(RESPONSE_LIMIT).read_to_end(&mut raw).ok()?;
    parse(&raw)
}

fn parse(raw: &[u8]) -> Option<Response> {
    // 本文は UTF-8 のはずだが、壊れていても落とさずに読めるところまで読む。
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = status_code(head.lines().next().unwrap_or_default())?;
    Some(Response {
        status,
        body: body.trim().to_string(),
    })
}

/// 状態行 (`HTTP/1.1 200 OK`) からステータスコードを取り出す。
fn status_code(line: &str) -> Option<u16> {
    let mut fields = line.split_whitespace();
    let version = fields.next()?;
    if !version.starts_with("HTTP/") {
        return None;
    }
    fields.next()?.parse().ok()
}

/// 1回だけ接続を受け、リクエストを読み捨てて `response` を返す相手を立てる。
/// 問い合わせる側 (`setup`) のテストから使う。
#[cfg(test)]
pub(super) fn serve_once(response: &'static str) -> SocketAddr {
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("ループバックに bind できるはず");
    let addr = listener
        .local_addr()
        .expect("bind したアドレスを取れるはず");
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("接続を受けられるはず");
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request);
        let _ = stream.write_all(response.as_bytes());
    });
    addr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_the_status_and_the_body() {
        let response = parse(b"HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\n\r\nabc123\n");
        assert_eq!(
            response,
            Some(Response {
                status: 200,
                body: "abc123".to_string(),
            })
        );
    }

    #[test]
    fn parse_reads_a_response_without_a_body() {
        let response = parse(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n");
        assert_eq!(
            response,
            Some(Response {
                status: 404,
                body: String::new(),
            })
        );
    }

    #[test]
    fn parse_rejects_responses_that_are_not_http() {
        assert_eq!(parse(b""), None);
        assert_eq!(parse(b"hello\r\n\r\n"), None);
        assert_eq!(parse(b"HTTP/1.1 nine\r\n\r\n"), None);
    }
}
