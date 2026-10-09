//! ブラウザーが直接開く経路のエラーを、人間向けの応答に変える。
//!
//! ファイルは `<a href target="_blank">` で API の URL をそのまま開く。
//! 失敗したときに JSON の envelope をそのまま返すと、タブに
//! `{"error":{"code":"unauthorized",...}}` が出るだけで、そこから戻る手がかりが無い。
//! 閲覧者にはタブレットの生徒も含まれるので、これは行き止まりになる。
//!
//! `Accept` が HTML を求めている要求にだけ、画面へのリダイレクトを返す。
//! `fetch` から呼ぶ API クライアントには今までどおり JSON を返す
//! (→ `docs/architecture.md`「エラーの契約と表示言語」)。

use axum::extract::{OriginalUri, Request};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

use crate::error::ErrorCode;

/// `Accept` が HTML を求めているか。
///
/// ブラウザーのトップレベルナビゲーションは `text/html` を候補の先頭に置く。
/// `fetch` の既定は `*/*` なので、`text/html` を名指ししているかどうかで見分けられる。
/// ワイルドカード (`*/*`) を HTML 扱いにしないのは、それを送るのが API クライアント側だから。
fn wants_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|accept| accept.split(',').any(entry_wants_html))
}

/// `Accept` の項目1つが HTML を求めているか。
///
/// - メディア型は**大文字小文字を区別しない** (RFC 9110)
/// - 前方一致では `text/html-sandboxed` のような別の型まで拾うので、型名は完全に一致させる
/// - **`q=0` は「受け付けない」**の意味なので除く。`q` が無ければ既定は 1
///   (`q` が数として読めないものは、受け付ける側に倒して HTML 扱いにする)
fn entry_wants_html(entry: &str) -> bool {
    let mut parts = entry.split(';').map(str::trim);
    if !parts
        .next()
        .is_some_and(|media_type| media_type.eq_ignore_ascii_case("text/html"))
    {
        return false;
    }
    !parts.any(|parameter| {
        parameter.split_once('=').is_some_and(|(name, value)| {
            name.trim().eq_ignore_ascii_case("q")
                && value.trim().parse::<f32>().is_ok_and(|q| q <= 0.0)
        })
    })
}

/// エラーの応答を、画面へ送るリダイレクトに置き換える。
///
/// 行き先はステータスだけで決め、応答のボディは読まない。
/// ボディから `code` を取るには本文を読み切る必要があるうえ、ここで分けたい粒度は
/// 「ログインへ送るか、伝えて終わるか」しかないため。
fn redirect_for(status: StatusCode, path_and_query: &str) -> Response {
    // 303 にするのは、元の要求が GET でなくてもリダイレクト先を GET で取らせるため。
    if status == StatusCode::UNAUTHORIZED {
        // ログインしたら開こうとしていたファイルへ戻す。
        let target = utf8_percent_encode(path_and_query, NON_ALPHANUMERIC);
        return Redirect::to(&format!("/login?redirect={target}")).into_response();
    }
    let reason = match status {
        StatusCode::FORBIDDEN => ErrorCode::Forbidden.to_string(),
        StatusCode::NOT_FOUND => ErrorCode::NotFound.to_string(),
        // それ以外は画面側で汎用の文言に落とす。ステータスごとに増やさない。
        _ => "generic".to_string(),
    };
    Redirect::to(&format!("/?error={reason}")).into_response()
}

/// ダウンロード系のルートにだけ掛ける。
pub(crate) async fn redirect_errors_for_browsers(request: Request, next: Next) -> Response {
    let redirect = wants_html(request.headers());
    // リダイレクト先に載せるため、応答を待つ前に控える (`Request` は `next` に渡すと戻らない)。
    //
    // `request.uri()` では足りない。`Router::nest` はネストしたルーターに渡す URI から
    // プレフィックス (`/api/v1`) を落とすため、そのままでは戻り先が
    // `/contents/1/download` になり、ログイン後に開けないパスを渡してしまう。
    // 元の URI は `nest` が `OriginalUri` として残している。
    let path_and_query = request
        .extensions()
        .get::<OriginalUri>()
        .and_then(|original| original.0.path_and_query().map(ToString::to_string))
        .or_else(|| request.uri().path_and_query().map(ToString::to_string))
        .unwrap_or_default();

    let response = next.run(request).await;
    let status = response.status();
    if !redirect || !(status.is_client_error() || status.is_server_error()) {
        return response;
    }
    redirect_for(status, &path_and_query)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(accept: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(accept) = accept {
            headers.insert(
                header::ACCEPT,
                accept.parse().expect("ヘッダー値にできなかった"),
            );
        }
        headers
    }

    /// ブラウザーがアドレスバーから開くときの `Accept`。
    #[test]
    fn navigation_accept_wants_html() {
        let accept = "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,*/*;q=0.8";
        assert!(wants_html(&headers(Some(accept))));
    }

    /// `fetch` の既定と、明示的に JSON を求める API クライアント。
    #[test]
    fn api_clients_do_not_want_html() {
        assert!(!wants_html(&headers(Some("*/*"))));
        assert!(!wants_html(&headers(Some("application/json"))));
        assert!(!wants_html(&headers(None)));
    }

    /// `q=0` は「受け付けない」の意味。名指しされていても HTML 扱いにしない。
    #[test]
    fn html_rejected_with_q_zero_does_not_count() {
        assert!(!wants_html(&headers(Some(
            "text/html;q=0, application/json"
        ))));
        assert!(!wants_html(&headers(Some("text/html;q=0.0, */*"))));
        // `q` が無ければ既定は 1。読めない値は受け付ける側に倒す。
        assert!(wants_html(&headers(Some("text/html;q=0.8"))));
        assert!(wants_html(&headers(Some("text/html;q=abc"))));
    }

    /// メディア型は大文字小文字を区別しない。前方一致だと別の型まで拾ってしまう。
    #[test]
    fn media_type_is_matched_exactly_and_case_insensitively() {
        assert!(wants_html(&headers(Some("TEXT/HTML"))));
        assert!(wants_html(&headers(Some("Text/Html; charset=utf-8"))));
        assert!(!wants_html(&headers(Some("text/html-sandboxed"))));
        assert!(!wants_html(&headers(Some("application/xhtml+xml"))));
    }

    /// `<audio>` や `<img>` が取りに来る場合もリダイレクトしない。
    /// 画面の中で鳴らす・表示するものなので、ページに飛ばしても意味がない。
    #[test]
    fn media_elements_do_not_want_html() {
        assert!(!wants_html(&headers(Some(
            "audio/webm,audio/ogg,audio/wav,audio/*;q=0.9"
        ))));
        assert!(!wants_html(&headers(Some("image/avif,image/webp,*/*"))));
    }

    fn location(response: &Response) -> &str {
        response
            .headers()
            .get(header::LOCATION)
            .expect("Location が無い")
            .to_str()
            .expect("Location を文字列にできなかった")
    }

    /// 401 はログインへ送り、開こうとしていたファイルへ戻せるようにする。
    #[test]
    fn unauthorized_goes_to_login_with_the_original_path() {
        let response = redirect_for(
            StatusCode::UNAUTHORIZED,
            "/api/v1/contents/12/download?path=a%2Fb.pdf",
        );
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            location(&response),
            "/login?redirect=%2Fapi%2Fv1%2Fcontents%2F12%2Fdownload%3Fpath%3Da%252Fb%2Epdf"
        );
    }

    #[test]
    fn other_errors_go_to_the_top_with_a_reason() {
        for (status, expected) in [
            (StatusCode::FORBIDDEN, "/?error=forbidden"),
            (StatusCode::NOT_FOUND, "/?error=not_found"),
            (StatusCode::INTERNAL_SERVER_ERROR, "/?error=generic"),
            (StatusCode::TOO_MANY_REQUESTS, "/?error=generic"),
        ] {
            let response = redirect_for(status, "/api/v1/contents/1/download");
            assert_eq!(response.status(), StatusCode::SEE_OTHER, "{status}");
            assert_eq!(location(&response), expected, "{status}");
        }
    }
}
