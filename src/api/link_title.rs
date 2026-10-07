//! リンクの作成・編集時に、URL のページからタイトルと説明を取る。
//! リンクのカードに出す情報 (画像・サイト名・公開日時・アイコン) と画像も、ここで取る (→ `link_preview`)。
//!
//! 取れなかったときは呼び出し側が判断する (タイトルなら URL のホスト名にする等)。
//! 取得の失敗で呼び出し元の処理を止めないため、ここでは理由を返さずフィールドごとに
//! `None` にまとめる。
//!
//! 取得先はログイン済みなら誰でも指定できる。サーバーと同じ LAN の機器を突くのに
//! 使われないよう、公開アドレスにだけ接続する (`PublicOnlyResolver`)。リダイレクト先にも
//! 同じ判定が掛かる。外向きの通信はこのモジュールだけに閉じる (URL のファイルの中継も、開くのはここ)。

use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use std::time::Duration;

use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};
use ureq::{Agent, ResponseExt};

use crate::error::{AppError, run_blocking};

/// 取得全体の時間切れ。作成のボタンを押してから待たされる時間の上限になる。
const TIMEOUT: Duration = Duration::from_secs(5);

/// 読む本文の上限。`og:title` と `<title>` は `<head>` にあるので、巨大なページは先頭だけを読む。
/// YouTube の動画のページは `<head>` の前に 700KB ほどのスクリプトを置くので、それが収まる大きさにする。
/// 同時に取りに行くのは数件 (`link_preview::MAX_CONCURRENT_FETCHES`) なので、メモリは数 MB に収まる。
const MAX_BODY_BYTES: u64 = 2 * 1024 * 1024;

/// タイトルの長さの上限 (文字数)。長い `<title>` (SEO 用の文言など) をそのまま入れない。
const MAX_TITLE_CHARS: usize = 200;

/// 説明の長さの上限 (文字数)。`og:description` はタイトルより長いことが多いため別に持つ。
const MAX_DESCRIPTION_CHARS: usize = 500;

const MAX_REDIRECTS: u32 = 3;

/// URL のページから取ったタイトルと説明。それぞれ取れなかった・空だったときは `None`。
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct LinkMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
}

/// ページのタイトルと説明を返す。取れない・読めないときはどちらも `None`。
pub(super) async fn fetch_link_metadata(url: &str) -> Result<LinkMetadata, AppError> {
    let url = url.to_string();
    run_blocking(move || fetch_link_metadata_blocking(&url)).await
}

fn fetch_link_metadata_blocking(url: &str) -> LinkMetadata {
    fetch_page(url).unwrap_or_default()
}

/// スキームが省かれた URL (`example.com/x`) に付けるスキームを決め、補った URL を返す。
/// 相手のページの中身も返す。取れなければ空のもの (試し終えているので、呼び出し側は取り直さない)。
/// - LAN の相手: 試さずに `http://`。LAN へは接続しない決まりで試せず、LAN の機器は http でしか応答しないものが多いため
/// - それ以外: `https://`、応答が無ければ `http://` を試す。どちらも無ければ `https://`
pub(super) async fn choose_scheme(
    without_scheme: &str,
) -> Result<(String, LinkMetadata), AppError> {
    let without_scheme = without_scheme.to_string();
    run_blocking(move || {
        if is_lan_host(&without_scheme) {
            return (format!("http://{without_scheme}"), LinkMetadata::default());
        }
        let (url, metadata) = choose_scheme_by_response(&without_scheme, fetch_page);
        (url, metadata.unwrap_or_default())
    })
    .await
}

fn choose_scheme_by_response(
    without_scheme: &str,
    fetch: impl Fn(&str) -> Option<LinkMetadata>,
) -> (String, Option<LinkMetadata>) {
    for scheme in ["https", "http"] {
        let url = format!("{scheme}://{without_scheme}");
        if let Some(metadata) = fetch(&url) {
            return (url, Some(metadata));
        }
    }
    (format!("https://{without_scheme}"), None)
}

/// ホストが LAN の相手か。IP ならそのアドレスで、名前なら `.local` か、引いた先がすべて公開アドレスでないかで決める。
/// 名前を引けなければ LAN とはみなさない。
fn is_lan_host(without_scheme: &str) -> bool {
    let Ok(uri) = format!("http://{without_scheme}").parse::<ureq::http::Uri>() else {
        return false;
    };
    let Some(host) = uri.host() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host.parse::<IpAddr>() {
        return !is_public(ip);
    }
    let name = host.trim_end_matches('.').to_ascii_lowercase();
    if name.ends_with(".local") {
        return true;
    }
    match (name.as_str(), 0).to_socket_addrs() {
        Ok(addrs) => {
            let addrs: Vec<_> = addrs.collect();
            !addrs.is_empty() && addrs.iter().all(|addr| !is_public(addr.ip()))
        }
        Err(_) => false,
    }
}

/// ページと画像を取る口。全体を `TIMEOUT` で切る。
fn agent() -> Agent {
    public_only_agent(Agent::config_builder().timeout_global(Some(TIMEOUT)))
}

/// 公開アドレスにだけつなぐ取得の口。外へつなぐ取得は、どれもこれを通す。
/// 時間切れの決め方だけを呼び出し側が `config` で渡す。
fn public_only_agent(config: ureq::config::ConfigBuilder<ureq::typestate::AgentScope>) -> Agent {
    let config = config
        .max_redirects(MAX_REDIRECTS)
        .http_status_as_error(false)
        // 環境変数のプロキシを使うと、接続先の判定が回避される。
        .proxy(None)
        .user_agent("WebLAV")
        .tls_config(super::native_tls_config())
        .build();
    Agent::with_parts(config, DefaultConnector::default(), PublicOnlyResolver)
}

/// ファイルの中継で、応答の頭が届くまでの上限。ページの取得 (`TIMEOUT`) より長くするのは、
/// 動画の配信元などが Range の頭を返すまでに時間の掛かることがあるため。
const FILE_RESPONSE_TIMEOUT: Duration = Duration::from_secs(15);

/// ファイルの中継で、本文を受け取り終えるまでの上限。閲覧側が読む速さで受け取るので、
/// 動画を見ている間はつながったままになる。止まった相手のためにスレッドを持ち続けないための上限で、
/// 切れても動画はブラウザが Range で続きを頼み直す。
const FILE_BODY_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// URL のファイルを中継するために開く (→ `remote_file`)。`range` は閲覧側の `Range` をそのまま渡す。
/// 応答が無ければ `None` (状態の判断は呼び出し側)。
pub(super) fn open_file_blocking(
    url: &str,
    range: Option<&str>,
) -> Option<ureq::http::Response<ureq::Body>> {
    let agent = public_only_agent(
        Agent::config_builder()
            .timeout_connect(Some(TIMEOUT))
            .timeout_recv_response(Some(FILE_RESPONSE_TIMEOUT))
            .timeout_recv_body(Some(FILE_BODY_TIMEOUT)),
    );
    let mut request = agent.get(url);
    if let Some(range) = range {
        request = request.header("Range", range);
    }
    request.call().ok()
}

/// ページを取り、タイトルと説明を返す。相手から応答が無ければ `None`
/// (応答があれば、エラーの状態や HTML でないときも中身が空の `Some`)。
fn fetch_page(url: &str) -> Option<LinkMetadata> {
    let mut response = agent().get(url).call().ok()?;
    if !response.status().is_success() {
        return Some(LinkMetadata::default());
    }
    let Some(html) = read_html(&mut response) else {
        return Some(LinkMetadata::default());
    };
    Some(extract_metadata(&html))
}

/// HTML の応答の本文を、先頭から `MAX_BODY_BYTES` まで読む。HTML でない・UTF-8 で読めないときは `None`。
fn read_html(response: &mut ureq::http::Response<ureq::Body>) -> Option<String> {
    let is_html = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("html"));
    if !is_html {
        return None;
    }
    // `limit` は超えると読んだ分まで捨ててエラーにするので、先頭から上限までを自分で切り取る。
    // 大きいページ (GitHub など) でも、`<head>` は先頭にある。
    let mut body = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_BODY_BYTES)
        .read_to_end(&mut body)
        .ok()?;
    utf8_prefix(body)
}

/// UTF-8 として読む。文字コードの変換はしない。UTF-8 として読めなければ、化けた題を作らず諦める。
/// ただし上限で切った末尾の、途中で切れた1文字は落として読む。
fn utf8_prefix(body: Vec<u8>) -> Option<String> {
    match String::from_utf8(body) {
        Ok(text) => Some(text),
        Err(err) if err.utf8_error().error_len().is_none() => {
            let valid = err.utf8_error().valid_up_to();
            let mut bytes = err.into_bytes();
            bytes.truncate(valid);
            String::from_utf8(bytes).ok()
        }
        Err(_) => None,
    }
}

/// リンクのカードに出す、ページの情報。URL は相手のページを基準に解決した、`http`・`https` の絶対 URL。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct PageCard {
    pub title: Option<String>,
    pub site_name: Option<String>,
    /// `article:published_time` のまま。形は相手しだいなので、読めるかは画面が決める。
    pub published_at: Option<String>,
    pub image_url: Option<String>,
    pub icon_url: Option<String>,
}

/// キャッシュの決まり (RFC 9111) に使う応答のヘッダー。値は相手が送ったまま。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct CacheHeaders {
    pub cache_control: Option<String>,
    pub expires: Option<String>,
    pub date: Option<String>,
    pub age: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

/// カードの情報を取り直した結果。
#[derive(Debug)]
pub(super) enum CardFetch {
    /// 前回から変わっていない (304)。
    NotModified(CacheHeaders),
    Fetched {
        card: PageCard,
        headers: CacheHeaders,
    },
    /// 応答が無い・エラーの状態だった。前回の情報を使い続ける。
    Failed,
}

/// カードの情報を取る。前回の検証子 (`ETag`・`Last-Modified`) があれば、条件付きで取り直す。
pub(super) fn fetch_card_blocking(
    url: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> CardFetch {
    let mut request = agent().get(url);
    if let Some(etag) = etag {
        request = request.header("If-None-Match", etag);
    }
    if let Some(last_modified) = last_modified {
        request = request.header("If-Modified-Since", last_modified);
    }
    let Ok(mut response) = request.call() else {
        return CardFetch::Failed;
    };
    let headers = cache_headers(response.headers());
    if response.status() == ureq::http::StatusCode::NOT_MODIFIED {
        return CardFetch::NotModified(headers);
    }
    if !response.status().is_success() {
        return CardFetch::Failed;
    }
    // 相対の URL は、リダイレクトを辿った後のページを基準に解決する。
    let base = url::Url::parse(&response.get_uri().to_string()).ok();
    let card = match (read_html(&mut response), base) {
        (Some(html), Some(base)) => extract_card(&html, &base),
        // HTML でないページ (PDF など) は、ホスト名とアイコンだけのカードになる。
        (_, base) => PageCard {
            icon_url: base.and_then(|base| default_icon(&base)),
            ..PageCard::default()
        },
    };
    CardFetch::Fetched { card, headers }
}

fn cache_headers(headers: &ureq::http::HeaderMap) -> CacheHeaders {
    let get = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    };
    CacheHeaders {
        cache_control: get("cache-control"),
        expires: get("expires"),
        date: get("date"),
        age: get("age"),
        etag: get("etag"),
        last_modified: get("last-modified"),
    }
}

/// 画像を `max_bytes` まで読む。画像でない・大きすぎる・取れないときは `None`。
pub(super) fn fetch_image_blocking(url: &str, max_bytes: u64) -> Option<Vec<u8>> {
    let mut response = agent().get(url).call().ok()?;
    if !response.status().is_success() {
        return None;
    }
    // `image/*` と言わないものも、中身で画像と分かれば使う (favicon.ico を別の型で返すサーバーがある)。
    // HTML などの明らかに画像でないものだけを弾く。
    let is_text = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("text/"));
    if is_text {
        return None;
    }
    response
        .body_mut()
        .with_config()
        .limit(max_bytes)
        .read_to_vec()
        .ok()
}

/// HTML からカードの情報を取り出す。
fn extract_card(html: &str, base: &url::Url) -> PageCard {
    let lower = html.to_ascii_lowercase();
    let image = extract_meta_content(html, &lower, "og:image", MAX_URL_CHARS)
        .or_else(|| extract_meta_content(html, &lower, "og:image:url", MAX_URL_CHARS))
        .or_else(|| extract_meta_content(html, &lower, "twitter:image", MAX_URL_CHARS));
    PageCard {
        title: extract_page_title(html, &lower),
        site_name: extract_meta_content(html, &lower, "og:site_name", MAX_TITLE_CHARS),
        published_at: extract_meta_content(html, &lower, "article:published_time", MAX_TITLE_CHARS),
        image_url: image.and_then(|image| resolve_web_url(base, &image)),
        icon_url: extract_icon_href(html, &lower)
            .and_then(|href| resolve_web_url(base, &href))
            .or_else(|| default_icon(base)),
    }
}

/// `og:image` やアイコンの URL の長さの上限 (文字数)。これより長いものは壊れているとみなす。
const MAX_URL_CHARS: usize = 2048;

/// 相手のページを基準に URL を解決する。`http`・`https` 以外 (`data:` など) は使わない。
fn resolve_web_url(base: &url::Url, href: &str) -> Option<String> {
    let url = base.join(href.trim()).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}

/// アイコンの指定が無いときに、ブラウザと同じく探す場所。
fn default_icon(base: &url::Url) -> Option<String> {
    resolve_web_url(base, "/favicon.ico")
}

/// `<link rel="icon">` の `href`。SVG は縮められないので飛ばし、`icon` の指定が無ければ
/// `apple-touch-icon` を使う。
fn extract_icon_href(html: &str, lower: &str) -> Option<String> {
    let mut touch_icon = None;
    for attrs in tags(html, lower, "link") {
        let attr = |name: &str| {
            attrs
                .iter()
                .find(|(attr, _)| attr == name)
                .map(|(_, value)| value.trim())
        };
        let Some(href) = attr("href").filter(|href| !href.is_empty()) else {
            continue;
        };
        let is_svg = attr("type").is_some_and(|ty| ty.eq_ignore_ascii_case("image/svg+xml"))
            || href
                .to_ascii_lowercase()
                .split(['?', '#'])
                .next()
                .is_some_and(|path| path.ends_with(".svg"));
        if is_svg {
            continue;
        }
        let rel = attr("rel").unwrap_or_default().to_ascii_lowercase();
        let mut tokens = rel.split_ascii_whitespace();
        if tokens.clone().any(|token| token == "icon") {
            return Some(decode_entities(href));
        }
        if touch_icon.is_none() && tokens.any(|token| token.starts_with("apple-touch-icon")) {
            touch_icon = Some(decode_entities(href));
        }
    }
    touch_icon
}

/// HTML からタイトルと説明を取り出す。本文の小文字化は 1 回だけ行い、各抽出で使い回す
/// (`<meta>`・`<title>` の位置探しに使う。長さは変わらないので位置は元の文字列でも合う)。
fn extract_metadata(html: &str) -> LinkMetadata {
    let lower = html.to_ascii_lowercase();
    LinkMetadata {
        title: extract_page_title(html, &lower),
        description: extract_page_description(html, &lower),
    }
}

/// 名前解決の結果から、公開アドレス以外を除く。
#[derive(Debug)]
struct PublicOnlyResolver;

impl Resolver for PublicOnlyResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let resolved = DefaultResolver::default().resolve(uri, config, timeout)?;
        // IPv4 を先に試す。ureq は先頭のアドレスに接続の持ち時間の大半を割くので、IPv6 に届かない
        // 回線で IPv6 が先に来ると、時間切れまで待ってから IPv4 を試す余裕が残らない。
        let mut kept = self.empty();
        let public = resolved.iter().filter(|addr| is_public(addr.ip()));
        for addr in public.clone().filter(|addr| addr.is_ipv4()) {
            kept.push(*addr);
        }
        for addr in public.filter(|addr| addr.is_ipv6()) {
            kept.push(*addr);
        }
        if kept.is_empty() {
            return Err(ureq::Error::HostNotFound);
        }
        Ok(kept)
    }
}

/// インターネット上の相手として接続してよいアドレスか。
/// `IpAddr::is_global` は安定版に無いため、除くものを自前で挙げる。
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_v4(ip),
        IpAddr::V6(ip) => match embedded_v4(ip) {
            Some(embedded) => is_public_v4(embedded),
            None => is_public_v6(ip),
        },
    }
}

/// IPv6 の中に IPv4 が入っている形から、その IPv4 を取り出す。
/// IPv4-mapped (`::ffff:a.b.c.d`)、IPv4-compatible (`::a.b.c.d`)、NAT64 (`64:ff9b::/96`)、
/// 6to4 (`2002:aabb:ccdd::/48`) は、届く先が中の IPv4 になる。
fn embedded_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let s = ip.segments();
    let v4_at = |hi: u16, lo: u16| {
        let [a, b] = hi.to_be_bytes();
        let [c, d] = lo.to_be_bytes();
        Ipv4Addr::new(a, b, c, d)
    };
    match s {
        [0, 0, 0, 0, 0, 0xffff, hi, lo] | [0, 0, 0, 0, 0, 0, hi, lo] => Some(v4_at(hi, lo)),
        [0x64, 0xff9b, 0, 0, 0, 0, hi, lo] => Some(v4_at(hi, lo)),
        [0x2002, hi, lo, ..] => Some(v4_at(hi, lo)),
        _ => None,
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        // 「このネットワーク」(0.0.0.0/8) と、予約済みの 240.0.0.0/4。
        || a == 0
        || a >= 240
        // 共有アドレス空間 (100.64.0.0/10)。プロバイダ内の機器に当たる。
        || (a == 100 && (64..128).contains(&b))
        // IETF の特別用途 (192.0.0.0/24) と、機器の試験用 (198.18.0.0/15)。
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && (b == 18 || b == 19)))
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    // グローバルユニキャスト (2000::/3) だけを許す。ユニークローカル・リンクローカル・
    // 廃止されたサイトローカル (fec0::/10) など、それ以外は一括で除く。
    ip.segments()[0] & 0xe000 == 0x2000
}

/// ページのタイトル。`og:title` があればそれ、無ければ `<title>`。
/// `og:title` はサイト名を除いた短いタイトルになっていることが多いため、先に見る。
fn extract_page_title(html: &str, lower: &str) -> Option<String> {
    extract_meta_content(html, lower, "og:title", MAX_TITLE_CHARS)
        .or_else(|| extract_title(html, lower))
}

/// ページの説明。`og:description` があればそれ、無ければ `<meta name="description">`。
fn extract_page_description(html: &str, lower: &str) -> Option<String> {
    extract_meta_content(html, lower, "og:description", MAX_DESCRIPTION_CHARS)
        .or_else(|| extract_meta_content(html, lower, "description", MAX_DESCRIPTION_CHARS))
}

/// `<meta property="{name}" content="...">` (または `name="{name}"`) の `content` を
/// 整形して返す。`name` に一致し中身が空でない最初の `<meta>` を採る (空の `og:*` があっても
/// フォールバック先を塞がないよう、整形して空になるものは読み飛ばす)。属性の並びと引用符は問わない。
fn extract_meta_content(html: &str, lower: &str, name: &str, max_chars: usize) -> Option<String> {
    tags(html, lower, "meta").find_map(|attrs| {
        let matches_name = attrs.iter().any(|(attr, value)| {
            (attr == "property" || attr == "name") && value.trim().eq_ignore_ascii_case(name)
        });
        if !matches_name {
            return None;
        }
        let content = attrs.into_iter().find(|(attr, _)| attr == "content")?.1;
        normalize_text(&content, max_chars)
    })
}

/// `<name ...>` のタグを順に探し、属性を返す。`lower` は `html` を小文字にしたもの。
fn tags<'a>(
    html: &'a str,
    lower: &'a str,
    name: &'a str,
) -> impl Iterator<Item = Vec<(String, String)>> + 'a {
    let open = format!("<{name}");
    let starts: Vec<usize> = lower
        .match_indices(open.as_str())
        .map(|(at, _)| at + open.len())
        .collect();
    starts
        .into_iter()
        .map_while(move |start| {
            // `to_ascii_lowercase` は長さを変えないので、位置は元の文字列でもそのまま使える。
            let tag = &html[start..];
            // `<metadata` のような別タグを弾く。名前の直後は空白・`>`・`/` のいずれか。
            if tag
                .chars()
                .next()
                .is_some_and(|c| !(c.is_ascii_whitespace() || matches!(c, '>' | '/')))
            {
                return Some(None);
            }
            // 閉じの無いタグより後ろには、閉じたタグも無い。
            let end = tag.find('>')?;
            Some(Some(parse_attributes(&tag[..end])))
        })
        .flatten()
}

/// タグの中の `名前=値` を順に取る。名前は小文字にする。値は `"..."`・`'...'`・引用符なしのどれか。
fn parse_attributes(tag: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let mut rest = tag.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
    while !rest.is_empty() {
        let name_end = rest
            .find(|c: char| c == '=' || c.is_whitespace() || c == '/')
            .unwrap_or(rest.len());
        let name = rest[..name_end].to_ascii_lowercase();
        rest = rest[name_end..].trim_start();
        let mut value = String::new();
        if let Some(after_eq) = rest.strip_prefix('=') {
            let after_eq = after_eq.trim_start();
            let (raw, tail) = match after_eq.chars().next() {
                Some(quote @ ('"' | '\'')) => {
                    let inner = &after_eq[1..];
                    match inner.find(quote) {
                        Some(end) => (&inner[..end], &inner[end + 1..]),
                        None => (inner, ""),
                    }
                }
                _ => {
                    let end = after_eq
                        .find(|c: char| c.is_whitespace())
                        .unwrap_or(after_eq.len());
                    (&after_eq[..end], &after_eq[end..])
                }
            };
            value = raw.to_string();
            rest = tail;
        }
        if !name.is_empty() {
            attrs.push((name, value));
        }
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
    }
    attrs
}

/// 実体参照を戻し、空白をつめて `max_chars` 文字で切る。空なら `None`。
fn normalize_text(raw: &str, max_chars: usize) -> Option<String> {
    let text = decode_entities(raw);
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return None;
    }
    Some(text.chars().take(max_chars).collect())
}

/// 最初の `<title>` の中身。実体参照を戻し、空白をつめる。
fn extract_title(html: &str, lower: &str) -> Option<String> {
    // `<titlefoo>` のような別のタグは読み飛ばし、`<title>` か `<title 属性>` だけを探す。
    let open = lower.match_indices("<title").map(|(at, _)| at).find(|at| {
        lower[at + "<title".len()..]
            .chars()
            .next()
            .is_some_and(|next| next == '>' || next.is_ascii_whitespace())
    })?;
    let start = open + lower[open..].find('>')? + 1;
    // `</titlefoo>` のような別の閉じタグは読み飛ばし、`</title>` か `</title ...>` で閉じる。
    let end = start
        + lower[start..]
            .match_indices("</title")
            .map(|(at, _)| at)
            .find(|at| {
                lower[start + at + "</title".len()..]
                    .chars()
                    .next()
                    .is_some_and(|next| next == '>' || next.is_ascii_whitespace())
            })?;
    normalize_text(&html[start..end], MAX_TITLE_CHARS)
}

/// 主な実体参照と、数値参照 (`&#8211;`・`&#x2013;`) を戻す。
/// `&amp;` は他の参照を二重に戻さないよう最後にする。
fn decode_entities(text: &str) -> String {
    decode_numeric_references(text)
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// `&#N;` と `&#xH;` を文字に戻す。文字にならない値は、そのまま残す。
fn decode_numeric_references(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("&#") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let decoded = after.split_once(';').and_then(|(digits, tail)| {
            let code = match digits.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => digits.parse().ok()?,
            };
            Some((char::from_u32(code)?, tail))
        });
        match decoded {
            Some((ch, tail)) => {
                out.push(ch);
                rest = tail;
            }
            None => {
                out.push_str("&#");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// URL のホスト名。タイトルを取れなかったときの代わりに使う。
pub(super) fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    let host = if let Some(bracketed) = host_port.strip_prefix('[') {
        // IPv6 は `[::1]:8080` の形。括弧の中だけがホスト。
        bracketed.split(']').next().unwrap_or(bracketed)
    } else {
        host_port.split(':').next().unwrap_or(host_port)
    };
    host.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // 抽出関数は本文の小文字化を呼び出し側から受け取る。テストでは毎回書かずに済ませる。
    fn page_title(html: &str) -> Option<String> {
        extract_page_title(html, &html.to_ascii_lowercase())
    }
    fn page_description(html: &str) -> Option<String> {
        extract_page_description(html, &html.to_ascii_lowercase())
    }
    fn title_element(html: &str) -> Option<String> {
        extract_title(html, &html.to_ascii_lowercase())
    }

    /// 上限で切った本文の末尾で、途中で切れた1文字だけを落とす。途中に壊れた字があるものは読まない。
    #[test]
    fn utf8_prefix_drops_only_a_character_cut_at_the_end() {
        let cut = "タイトル".as_bytes()[..10].to_vec();
        assert_eq!(utf8_prefix(cut).as_deref(), Some("タイト"));
        assert_eq!(utf8_prefix(vec![b'a', 0xff, b'b']), None);
    }

    fn card(html: &str) -> PageCard {
        extract_card(
            html,
            &url::Url::parse("https://news.example/articles/1").expect("URL"),
        )
    }

    #[test]
    fn extract_card_resolves_relative_urls_against_the_page() {
        let card = card(
            r#"<head><meta property="og:image" content="/img/a.jpg?x=1&amp;y=2">
               <meta property="og:site_name" content="Example News">
               <meta property="article:published_time" content="2026-10-05T09:00:00+09:00">
               <link rel="shortcut icon" href="favicon.png"><title>Hello</title></head>"#,
        );
        assert_eq!(card.title.as_deref(), Some("Hello"));
        assert_eq!(card.site_name.as_deref(), Some("Example News"));
        assert_eq!(
            card.published_at.as_deref(),
            Some("2026-10-05T09:00:00+09:00")
        );
        assert_eq!(
            card.image_url.as_deref(),
            Some("https://news.example/img/a.jpg?x=1&y=2")
        );
        assert_eq!(
            card.icon_url.as_deref(),
            Some("https://news.example/articles/favicon.png")
        );
    }

    /// 縮められない SVG と、`http`・`https` 以外の URL は使わない。アイコンの指定が無ければ `/favicon.ico` を探す。
    #[test]
    fn extract_card_skips_unusable_images_and_icons() {
        let skipped = card(
            r#"<meta property="og:image" content="data:image/png;base64,AAAA">
               <link rel="icon" type="image/svg+xml" href="/icon.svg">
               <link rel="icon" href="/icon.svg?v=2">"#,
        );
        assert_eq!(skipped.image_url, None);
        assert_eq!(
            skipped.icon_url.as_deref(),
            Some("https://news.example/favicon.ico")
        );

        let touch = card(r#"<link rel="apple-touch-icon" sizes="180x180" href="/apple.png">"#);
        assert_eq!(
            touch.icon_url.as_deref(),
            Some("https://news.example/apple.png")
        );
    }

    /// 実際に HTTPS で取れること。ネットワークが要るので、手元で `--ignored` を付けて流す。
    #[test]
    #[ignore = "needs network access"]
    fn the_title_of_a_real_https_page_is_fetched() {
        assert_eq!(
            fetch_link_metadata_blocking("https://example.com/")
                .title
                .as_deref(),
            Some("Example Domain")
        );
    }

    /// 実際に https・http の順に試すこと。ネットワークが要るので、手元で `--ignored` を付けて流す。
    #[test]
    #[ignore = "needs network access"]
    fn the_scheme_of_a_real_site_is_chosen_by_its_response() {
        let choose = |without_scheme| choose_scheme_by_response(without_scheme, fetch_page).0;
        assert_eq!(choose("example.com/"), "https://example.com/");
        // httpforever.com は http でしか応答しない。
        assert_eq!(choose("httpforever.com/"), "http://httpforever.com/");
    }

    #[test]
    fn lan_hosts_are_told_apart_without_connecting() {
        assert!(is_lan_host("192.168.0.10/wiki"));
        assert!(is_lan_host("10.0.0.1:8080"));
        assert!(is_lan_host("[::1]:3000/x"));
        assert!(is_lan_host("nas.local:5000"));
        assert!(is_lan_host("NAS.local./"));
        assert!(is_lan_host("localhost:8080"));
        assert!(!is_lan_host("8.8.8.8/"));
        assert!(!is_lan_host("[2001:4860:4860::8888]/"));
    }

    fn metadata(title: &str) -> LinkMetadata {
        LinkMetadata {
            title: Some(title.to_string()),
            description: None,
        }
    }

    #[test]
    fn https_is_chosen_when_it_responds() {
        let chosen = choose_scheme_by_response("example.com/x", |_| Some(metadata("t")));
        assert_eq!(
            chosen,
            ("https://example.com/x".to_string(), Some(metadata("t")))
        );
    }

    #[test]
    fn http_is_chosen_when_only_it_responds() {
        let chosen = choose_scheme_by_response("example.com", |url| {
            url.starts_with("http://").then(|| metadata("t"))
        });
        assert_eq!(
            chosen,
            ("http://example.com".to_string(), Some(metadata("t")))
        );
    }

    #[test]
    fn https_is_chosen_when_neither_responds() {
        let chosen = choose_scheme_by_response("example.com", |_| None);
        assert_eq!(chosen, ("https://example.com".to_string(), None));
    }

    #[test]
    fn a_loopback_address_is_never_fetched() {
        assert_eq!(
            fetch_link_metadata_blocking("http://127.0.0.1:9/"),
            LinkMetadata::default()
        );
        assert_eq!(
            fetch_link_metadata_blocking("http://localhost/"),
            LinkMetadata::default()
        );
    }

    #[test]
    fn the_title_is_taken_from_the_title_element() {
        let html = "<html><head><TITLE lang=ja>\n  A &amp; B \n  C </TITLE></head></html>";
        assert_eq!(title_element(html).as_deref(), Some("A & B C"));
    }

    #[test]
    fn numeric_character_references_are_decoded() {
        let html = "<title>Foo &#8211; Bar &#x300C;x&#X300D; &#xZZ; &nbsp;end</title>";
        assert_eq!(
            title_element(html).as_deref(),
            Some("Foo – Bar 「x」 &#xZZ; end")
        );
    }

    #[test]
    fn a_tag_that_only_starts_with_title_is_skipped() {
        let html = "<titlebar>x</titlebar><title>real</title>";
        assert_eq!(title_element(html).as_deref(), Some("real"));
    }

    #[test]
    fn a_closing_tag_that_only_starts_with_title_does_not_cut_the_title() {
        let html = "<title>A </titlefoo> B</title>";
        assert_eq!(title_element(html).as_deref(), Some("A </titlefoo> B"));
    }

    #[test]
    fn a_tag_that_only_starts_with_meta_is_skipped() {
        // `<metadata>` は別タグ。meta として拾わない。
        let html = r#"<metadata name="description" content="nope">
            <meta name="description" content="real">"#;
        assert_eq!(page_description(html).as_deref(), Some("real"));
    }

    #[test]
    fn og_title_is_preferred_over_the_title_element() {
        let html = r#"<head><title>Site - Page</title>
            <meta content='Page &amp; more' property=og:title><meta name="x" content="y"></head>"#;
        assert_eq!(page_title(html).as_deref(), Some("Page & more"));
        let reversed = r#"<META PROPERTY="og:title" CONTENT="Big" />"#;
        assert_eq!(page_title(reversed).as_deref(), Some("Big"));
    }

    #[test]
    fn the_title_element_is_used_without_og_title() {
        let html = r#"<meta property="og:description" content="d"><title>T</title>"#;
        assert_eq!(page_title(html).as_deref(), Some("T"));
        let empty = r#"<meta property="og:title" content="  "><title>T</title>"#;
        assert_eq!(page_title(empty).as_deref(), Some("T"));
    }

    #[test]
    fn an_empty_leading_og_title_falls_back_to_a_later_valid_one() {
        // og:title が重複し、先頭が空でも、後続の有効な og:title を採る。
        let html = r#"<meta property="og:title" content="  ">
            <meta property="og:title" content="Real Title"><title>Site</title>"#;
        assert_eq!(page_title(html).as_deref(), Some("Real Title"));
    }

    #[test]
    fn og_description_is_preferred_over_the_meta_description() {
        let html = r#"<meta name="description" content="fallback">
            <meta property="og:description" content="Page &amp; more">"#;
        assert_eq!(page_description(html).as_deref(), Some("Page & more"));
    }

    #[test]
    fn the_meta_description_is_used_without_og_description() {
        let html = r#"<meta name="description" content="plain description">"#;
        assert_eq!(page_description(html).as_deref(), Some("plain description"));
    }

    #[test]
    fn an_empty_og_description_falls_back_to_the_meta_description() {
        // 空の og:description があっても、meta description へのフォールバックを塞がない。
        let html = r#"<meta property="og:description" content="  ">
            <meta name="description" content="real description">"#;
        assert_eq!(page_description(html).as_deref(), Some("real description"));
    }

    #[test]
    fn a_page_without_any_description_gives_none() {
        assert_eq!(page_description("<html><body>x</body></html>"), None);
        assert_eq!(
            page_description(r#"<meta name="description" content="  ">"#),
            None
        );
    }

    #[test]
    fn a_page_without_a_title_gives_none() {
        assert_eq!(title_element("<html><body>x</body></html>"), None);
        assert_eq!(title_element("<title>  </title>"), None);
    }

    #[test]
    fn a_long_title_is_cut() {
        let html = format!("<title>{}</title>", "あ".repeat(MAX_TITLE_CHARS + 50));
        assert_eq!(
            title_element(&html).map(|title| title.chars().count()),
            Some(MAX_TITLE_CHARS)
        );
    }

    #[test]
    fn the_host_drops_scheme_credentials_port_and_path() {
        assert_eq!(host_of("https://example.com/a?b#c"), "example.com");
        assert_eq!(host_of("http://user:pw@example.com:8080/x"), "example.com");
        assert_eq!(host_of("http://[::1]:8080/x"), "::1");
    }

    #[test]
    fn only_public_addresses_are_allowed() {
        for private in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.1.1",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:192.168.1.1",
            "::192.168.1.1",
            "64:ff9b::7f00:1",
            "2002:c0a8:101::1",
            "0.1.2.3",
            "240.0.0.1",
            "fec0::1",
            "fc00::1",
            "192.0.0.8",
            "198.18.0.1",
        ] {
            assert!(!is_public(private.parse().unwrap()), "{private}");
        }
        for public in ["8.8.8.8", "93.184.216.34", "2606:2800:220:1::1"] {
            assert!(is_public(public.parse().unwrap()), "{public}");
        }
    }
}
