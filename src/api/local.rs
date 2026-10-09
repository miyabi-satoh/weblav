//! この PC の中からの要求だけを通す extractor。
//!
//! 初回セットアップ (→ docs/access.md「初回セットアップ」) と「公開できるフォルダー」の管理 (→ docs/folders.md「公開できるフォルダー」) が使う。
//! どちらも、サーバーの PC の前にいる人だけに許す操作。

use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use axum::http::{HeaderMap, header};

use crate::error::AppError;

/// この PC の中からの要求だけを通す extractor。ハンドラの引数の先頭に置くと、
/// ボディを読む前に 404 で弾ける (LAN の端末には、この口が JSON を受けることも見せない)。
///
/// 接続元と宛先 (`Host`) の両方がループバックであることを見る。接続元だけでは足りない
/// (→ docs/access.md「初回セットアップ」)。
/// - 同じ PC の前段にリバースプロキシを置かれると (→ docs/architecture.md「LAN からの到達性」)、LAN からの要求もループバックで届く。
/// - 名前を 127.0.0.1 に振り直したページ (DNS リバインディング) は、ブラウザーから見て
///   同一オリジンになり、トークンの応答を読めてしまう。
///
/// プロキシが転送した印のある要求も通さない。LAN の端末が `Host: localhost` を名乗ると、
/// プロキシ越しでも接続元と `Host` の両方がループバックになるため。
///
/// トレイがブラウザーで開くのは `localhost:<port>`、問い合わせるのは `127.0.0.1:<port>` なので、
/// 正規の経路はどちらの条件も満たす。
pub(super) struct LocalRequest;

impl<S: Send + Sync> FromRequestParts<S> for LocalRequest {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // `ConnectInfo` を載せるのはサーバー本体だけ (→ `server::spawn`)。
        // 載っていなければ接続元が分からないということなので、通さない。
        let peer_is_loopback = matches!(
            parts.extensions.get::<ConnectInfo<SocketAddr>>(),
            Some(ConnectInfo(addr)) if is_loopback(*addr)
        );
        // HTTP/1.1 は `Host` ヘッダ、HTTP/2 は `:authority` (hyper が URI に入れる)。
        let host = parts
            .uri
            .authority()
            .map(|authority| authority.as_str())
            .or_else(|| {
                parts
                    .headers
                    .get(header::HOST)
                    .and_then(|value| value.to_str().ok())
            });

        if peer_is_loopback && host.is_some_and(is_loopback_host) && !is_forwarded(&parts.headers) {
            Ok(LocalRequest)
        } else {
            Err(AppError::NotFound)
        }
    }
}

/// プロキシが転送した印があるか。Caddy は既定で `X-Forwarded-For` を付ける。
/// nginx は既定では付けないので、マニュアルで足すよう案内する。
fn is_forwarded(headers: &HeaderMap) -> bool {
    [header::FORWARDED.as_str(), "x-forwarded-for", "x-real-ip"]
        .into_iter()
        .any(|name| headers.contains_key(name))
}

/// `::ffff:127.0.0.1` のような IPv4 射影アドレスは `Ipv6Addr::is_loopback` が false を
/// 返すため、IPv4 に戻してから見る。
fn is_loopback(addr: SocketAddr) -> bool {
    addr.ip().to_canonical().is_loopback()
}

/// `Host` がループバックを指すか (`127.0.0.1:3000`・`localhost`・`[::1]:3000` など)。
fn is_loopback_host(host: &str) -> bool {
    let host = host_without_port(host);
    let host = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host);
    match host.parse::<IpAddr>() {
        Ok(addr) => addr.to_canonical().is_loopback(),
        // 名前で引けるループバックは `localhost` だけとみなす。ほかの名前は、
        // 127.0.0.1 に向いていても DNS リバインディングと区別が付かない。
        Err(_) => host.eq_ignore_ascii_case("localhost"),
    }
}

/// `Host` からポートを落とす。IPv6 は `[::1]:3000` の形なので、角括弧の後ろだけを見る。
fn host_without_port(host: &str) -> &str {
    if let Some(end) = host.rfind(']') {
        return &host[..=end];
    }
    match host.split_once(':') {
        Some((name, _port)) => name,
        None => host,
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    fn peer(addr: &str) -> SocketAddr {
        addr.parse().expect("アドレスを解釈できなかった")
    }

    #[test]
    fn loopback_peers_are_allowed() {
        assert!(is_loopback(peer("127.0.0.1:50000")));
        // ループバックは 127.0.0.0/8 全体。
        assert!(is_loopback(peer("127.0.0.53:50000")));
        assert!(is_loopback(peer("[::1]:50000")));
        // IPv4 射影アドレス。dual-stack の待ち受けではこの形で届く。
        assert!(is_loopback(peer("[::ffff:127.0.0.1]:50000")));
    }

    #[test]
    fn loopback_hosts_are_allowed() {
        assert!(is_loopback_host("127.0.0.1:3000"));
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("localhost:3000"));
        assert!(is_loopback_host("LocalHost"));
        assert!(is_loopback_host("[::1]:3000"));
    }

    /// 前段のリバースプロキシ越し・DNS リバインディングは、どちらもここで落ちる。
    #[test]
    fn non_loopback_hosts_are_rejected() {
        assert!(!is_loopback_host("192.168.1.5:3000"));
        assert!(!is_loopback_host("weblav.example:3000"));
        // 127.0.0.1 に向けられていても、名前では通さない。
        assert!(!is_loopback_host("evil.example"));
        assert!(!is_loopback_host(""));
        assert!(!is_loopback_host("[2001:db8::1]:3000"));
    }

    #[test]
    fn forwarded_requests_are_detected() {
        for name in ["forwarded", "x-forwarded-for", "X-Real-IP"] {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::HeaderName::from_bytes(name.as_bytes()).expect("ヘッダ名を作れなかった"),
                "192.168.1.10".parse().expect("ヘッダの値を作れなかった"),
            );
            assert!(is_forwarded(&headers), "{name}");
        }
        assert!(!is_forwarded(&HeaderMap::new()));
    }

    /// 接続元・`Host`・転送の印を組み合わせた判定。どれか1つでも外れれば 404 にする。
    #[tokio::test]
    async fn local_request_requires_a_loopback_peer_and_host_without_forwarding() {
        let cases = [
            (
                "トレイが開く経路",
                Some("127.0.0.1:50000"),
                "127.0.0.1:3000",
                None,
                true,
            ),
            (
                "localhost 宛て",
                Some("[::1]:50000"),
                "localhost:3000",
                None,
                true,
            ),
            (
                "LAN の端末",
                Some("192.168.1.10:50000"),
                "192.168.1.5:3000",
                None,
                false,
            ),
            (
                "LAN の端末が Host を偽る",
                Some("192.168.1.10:50000"),
                "localhost:3000",
                None,
                false,
            ),
            ("接続元が分からない", None, "127.0.0.1:3000", None, false),
            // 前段のリバースプロキシ越し・DNS リバインディング。
            (
                "Host が LAN の名前",
                Some("127.0.0.1:50000"),
                "192.168.1.5:3000",
                None,
                false,
            ),
            (
                "Host がほかの名前",
                Some("127.0.0.1:50000"),
                "evil.example:3000",
                None,
                false,
            ),
            (
                "プロキシが X-Forwarded-For を付けた",
                Some("127.0.0.1:50000"),
                "localhost:3000",
                Some(("x-forwarded-for", "192.168.1.10")),
                false,
            ),
            (
                "プロキシが Forwarded を付けた",
                Some("127.0.0.1:50000"),
                "localhost:3000",
                Some(("forwarded", "for=192.168.1.10")),
                false,
            ),
        ];
        for (label, peer_addr, host, forwarded, allowed) in cases {
            let mut request = axum::http::Request::builder().header(header::HOST, host);
            if let Some((name, value)) = forwarded {
                request = request.header(name, value);
            }
            let (mut parts, ()) = request
                .body(())
                .expect("リクエストを組み立てられなかった")
                .into_parts();
            if let Some(addr) = peer_addr {
                parts.extensions.insert(ConnectInfo(peer(addr)));
            }

            let result = LocalRequest::from_request_parts(&mut parts, &()).await;

            match result {
                Ok(LocalRequest) => assert!(allowed, "{label}: 通してしまった"),
                Err(error) => {
                    assert!(!allowed, "{label}: 通さなかった");
                    assert!(matches!(error, AppError::NotFound), "{label}: {error:?}");
                }
            }
        }
    }

    #[test]
    fn non_loopback_peers_are_rejected() {
        assert!(!is_loopback(peer("192.168.1.10:50000")));
        assert!(!is_loopback(peer("[fe80::1]:50000")));
        assert!(!is_loopback(peer(&format!(
            "{}:50000",
            Ipv4Addr::new(128, 0, 0, 1)
        ))));
        assert!(!is_loopback(peer(&format!(
            "[{}]:50000",
            Ipv6Addr::UNSPECIFIED
        ))));
    }
}
