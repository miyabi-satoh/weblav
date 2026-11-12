//! URL のファイルを中継し、ファイルと同じビューアーで開けるようにする (→ docs/ui.md「URL のファイル」)。
//!
//! 中継するのは `link` コンテンツに登録された URL だけで、閲覧の権限を確かめてから取りに行く。
//! 誰でも任意の URL を取らせられる口にしないため。外へつなぐのは `link_title` の取得の口
//! (公開アドレスにだけつなぐ) に任せる。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use tokio::sync::{Semaphore, SemaphorePermit};
use tokio_util::io::{ReaderStream, SyncIoBridge};
use utoipa::IntoParams;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::completed_reader::CompletedReader;
use super::contents::{self, ContentType};
use super::link_title;
use super::links_file::{self, LinksFileQuery};
use crate::auth::Viewer;
use crate::error::{AppError, run_blocking};
use crate::state::AppState;

/// 相手から受け取って閲覧側へ送るまでの溜めの大きさ。閲覧側が読まない間 (動画の一時停止など) は
/// 相手からの受け取りも止まり、これより多くはメモリに載せない。
const RELAY_BUFFER_BYTES: usize = 256 * 1024;

/// 同時に中継する数の上限。中継は受け取り終えるまで blocking のスレッドを1本持つので、
/// 開いたままの接続でスレッドを使い切られないよう絞る。教室の 40 台ほどが一斉に動画を開き、
/// それぞれが Range で2本ずつつないでも足り、tokio の blocking のスレッドの既定 (512) には届かない数。
const MAX_CONCURRENT_RELAYS: usize = 128;

static RELAY_PERMITS: Semaphore = Semaphore::const_new(MAX_CONCURRENT_RELAYS);

/// 相手の応答から、そのまま閲覧側へ渡すヘッダー。Range と、ブラウザーが取り直しを判断するもの。
/// `Content-Type` は渡さず、こちらでファイル名の拡張子から決める (→ `relay_response`)。
const PASSED_HEADERS: [header::HeaderName; 5] = [
    header::CONTENT_LENGTH,
    header::CONTENT_RANGE,
    header::ACCEPT_RANGES,
    header::ETAG,
    header::LAST_MODIFIED,
];

/// `link` コンテンツの URL のファイルを、取りに行って流す。
/// 中継しない URL (開き方が決まらない・リンクでない) と、相手が応答しない・エラーを返すときは 404。
#[utoipa::path(
    get,
    path = "/contents/{id}/remote",
    params(("id" = i64, Path)),
    responses(
        (status = OK, description = "URL のファイルの中身"),
        (status = 206, description = "Range で頼まれた部分"),
        (status = 401, body = crate::error::ErrorResponse, description = "閲覧にログインが必要"),
        (status = 404, body = crate::error::ErrorResponse, description = "見えない・存在しない・中継しない URL、または相手から取れない"),
        (status = 416, description = "相手が Range を満たせなかった"),
        (status = 429, body = crate::error::ErrorResponse, description = "同時に中継している数が上限に達した"),
    )
)]
async fn remote_content(
    viewer: Viewer,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let row = sqlx::query!(
        r#"SELECT type as "content_type: ContentType", url FROM contents WHERE id = ?"#,
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    contents::ensure_viewable(&state.pool, &viewer, id).await?;
    if row.content_type != ContentType::Link {
        return Err(AppError::NotFound);
    }
    let url = row
        .url
        .ok_or(AppError::DataIntegrity("link content without a url"))?;
    relay(&state, url, &headers).await
}

/// リンクの一覧のファイル (`.links.toml`) の中のリンクの URL のファイルを、取りに行って流す。
#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
struct LinksRemoteQuery {
    /// 一覧のファイルの指し方 (`GET /contents/{id}/links` と同じ)。
    #[serde(default)]
    path: String,
    item: Option<i64>,
    /// 一覧に書かれたリンクの URL (`GET /contents/{id}/links` が返したまま)。
    url: String,
}

/// 一覧のファイルの中のリンクの URL のファイルを、取りに行って流す (→ docs/ui.md「URL のファイル」)。
/// 中継するのは、閲覧できる一覧に書かれた URL だけ。誰でも任意の URL を取らせられる口にしないため。
#[utoipa::path(
    get,
    path = "/contents/{id}/links/remote",
    params(("id" = i64, Path), LinksRemoteQuery),
    responses(
        (status = OK, description = "URL のファイルの中身"),
        (status = 206, description = "Range で頼まれた部分"),
        (status = 401, body = crate::error::ErrorResponse, description = "閲覧にログインが必要"),
        (status = 404, body = crate::error::ErrorResponse, description = "見えない・存在しない一覧、一覧に無い・中継しない URL、または相手から取れない"),
        (status = 416, description = "相手が Range を満たせなかった"),
        (status = 429, body = crate::error::ErrorResponse, description = "同時に中継している数が上限に達した"),
    )
)]
async fn links_remote_content(
    viewer: Viewer,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<LinksRemoteQuery>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let file = LinksFileQuery {
        path: query.path,
        item: query.item,
    };
    let urls = links_file::viewable_urls(&state, &viewer, id, &file).await?;
    if !urls.contains(&query.url) {
        return Err(AppError::NotFound);
    }
    relay(&state, query.url, &headers).await
}

/// 中継する URL を、公開アドレスにだけつないで取り、閲覧側の `Range` を渡して流す。
/// URL の拡張子で決まらなければ、リンクの情報を取ったときに相手の応答から決めたファイル名を使う
/// (→ `file_name_from_response`)。
async fn relay(state: &AppState, url: String, headers: &HeaderMap) -> Result<Response, AppError> {
    let (file_name, decided_by_response) = match relayed_file_name(&url) {
        Some(name) => (name, false),
        None => (
            super::link_preview::stored_file_name(&state.pool, &url)
                .await?
                .ok_or(AppError::NotFound)?,
            true,
        ),
    };

    // 待たせずに断る。ブラウザーの動画・音声は、読めなければ画面が案内を出す。
    let permit = RELAY_PERMITS
        .try_acquire()
        .map_err(|_| AppError::TooManyRequests)?;
    let range = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let upstream = run_blocking(move || link_title::open_file_blocking(&url, range.as_deref()))
        .await?
        .ok_or(AppError::NotFound)?;
    // 応答で決めた名前は、覚えた後に相手が中身を替えている (ページになったなど) ことがあるので、今の応答でも対象かを見る。
    if decided_by_response && link_title::response_file_name(&upstream).is_none() {
        return Err(AppError::NotFound);
    }
    relay_response(upstream, &file_name, permit)
}

/// URL を中継するなら、そのファイル名 (パスの最後の部分を戻したもの)。
/// 拡張子で開き方が決まるファイル (画面がビューアー・プレイヤーで開くもの) だけを中継する。
pub(super) fn relayed_file_name(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let last = url.path_segments()?.next_back()?;
    let name = percent_encoding::percent_decode_str(last)
        .decode_utf8()
        .ok()?
        .into_owned();
    super::text_files::kind_by_extension(&name).then_some(name)
}

/// 相手の `Content-Type` から決める拡張子。ビューアー・プレイヤーで開く種類だけで、HTML・SVG・XML のような
/// このアプリのオリジンでスクリプトが動きうるものは入れない (→ `relay_response`)。
const EXTENSION_BY_CONTENT_TYPE: [(&str, &str); 30] = [
    ("application/pdf", "pdf"),
    ("video/mp4", "mp4"),
    ("video/x-m4v", "m4v"),
    ("video/webm", "webm"),
    ("video/quicktime", "mov"),
    ("audio/mpeg", "mp3"),
    ("audio/mp4", "m4a"),
    ("audio/x-m4a", "m4a"),
    ("audio/aac", "aac"),
    ("audio/wav", "wav"),
    ("audio/x-wav", "wav"),
    ("audio/ogg", "ogg"),
    ("audio/flac", "flac"),
    ("audio/x-flac", "flac"),
    ("audio/opus", "opus"),
    ("image/jpeg", "jpg"),
    ("image/png", "png"),
    ("image/gif", "gif"),
    ("image/webp", "webp"),
    ("image/bmp", "bmp"),
    ("image/heic", "heic"),
    ("image/avif", "avif"),
    ("image/tiff", "tiff"),
    (
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "docx",
    ),
    (
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xlsx",
    ),
    (
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "pptx",
    ),
    ("text/plain", "txt"),
    ("text/markdown", "md"),
    ("text/x-markdown", "md"),
    ("text/csv", "csv"),
];

/// URL の拡張子で開き方が決まらないときに、相手の応答から決めたファイル名 (→ docs/ui.md「URL のファイル」)。
/// `final_url` はリダイレクトを辿った後の URL。
/// 拡張子の無いダウンロードの口 (`/download?id=1`) も、ファイルと同じビューアーで開けるようにするため。
/// - `Content-Disposition` のファイル名か、辿った後の URL の拡張子で種類が決まれば、その名前
/// - 決まらなければ、`Content-Type` が対象の種類なら、その名前 (無ければパスの最後の部分かホスト名) に拡張子を足す
/// - テキストは、ビューアーが読める大きさを超えると分かっていれば外す (新しいタブで開く)
pub(super) fn file_name_from_response(
    final_url: &str,
    content_type: Option<&str>,
    content_disposition: Option<&str>,
    content_length: Option<u64>,
) -> Option<String> {
    let disposition_name = content_disposition.and_then(disposition_file_name);
    if let Some(name) = disposition_name
        .clone()
        .filter(|name| super::text_files::kind_by_extension(name))
        .or_else(|| relayed_file_name(final_url))
    {
        return Some(name);
    }
    let essence = content_type?
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let (_, extension) = EXTENSION_BY_CONTENT_TYPE
        .iter()
        .find(|(content_type, _)| *content_type == essence)?;
    if essence.starts_with("text/")
        && content_length.is_some_and(|length| length > super::text_files::TEXT_VIEWER_MAX_BYTES)
    {
        return None;
    }
    let base = disposition_name.or_else(|| last_path_segment(final_url))?;
    // 名前の拡張子が同じ大分類 (text・video など) の対象なら、そのまま使う。`text/plain` で配る `.md` を
    // Markdown として開けるように。違えば、型の拡張子を足す。
    let top_level = |content_type: &str| {
        content_type
            .split('/')
            .next()
            .unwrap_or_default()
            .to_string()
    };
    let keeps_name = base.rsplit_once('.').is_some_and(|(_, ext)| {
        EXTENSION_BY_CONTENT_TYPE
            .iter()
            .any(|(content_type, known)| {
                known.eq_ignore_ascii_case(ext) && top_level(content_type) == top_level(&essence)
            })
    });
    Some(if keeps_name {
        base
    } else {
        format!("{base}.{extension}")
    })
}

/// `Content-Disposition` のファイル名。`filename*` (RFC 5987) を `filename` より先に使い、フォルダーの部分は落とす。
fn disposition_file_name(value: &str) -> Option<String> {
    let params = disposition_params(value);
    let extended = params
        .iter()
        .find(|(name, _)| name == "filename*")
        .and_then(|(_, value)| {
            let (charset, rest) = value.split_once('\'')?;
            let (_, encoded) = rest.split_once('\'')?;
            if !charset.eq_ignore_ascii_case("utf-8") {
                return None;
            }
            percent_encoding::percent_decode_str(encoded)
                .decode_utf8()
                .ok()
                .map(std::borrow::Cow::into_owned)
        });
    let name = extended.or_else(|| {
        params
            .into_iter()
            .find(|(name, _)| name == "filename")
            .map(|(_, value)| value)
    })?;
    let name = name.rsplit(['/', '\\']).next().unwrap_or_default().trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// `Content-Disposition` の型の後ろの引数 (名前は小文字)。引用の中の `;` では区切らず、引用は `\` の逃がしを戻して外す (RFC 6266・RFC 9110 の quoted-string)。
fn disposition_params(value: &str) -> Vec<(String, String)> {
    let mut params = Vec::new();
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                current.push(c);
            }
            '\\' if quoted => {
                current.push(c);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ';' if !quoted => segments.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    segments.push(current);
    for segment in segments.into_iter().skip(1) {
        let Some((name, raw)) = segment.split_once('=') else {
            continue;
        };
        let raw = raw.trim();
        let value = match raw.strip_prefix('"') {
            Some(inner) => {
                let mut unquoted = String::new();
                let mut chars = inner.chars();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => unquoted.extend(chars.next()),
                        '"' => break,
                        _ => unquoted.push(c),
                    }
                }
                unquoted
            }
            None => raw.to_string(),
        };
        params.push((name.trim().to_ascii_lowercase(), value));
    }
    params
}

/// URL のパスの最後の部分を戻したもの。空ならホスト名。
fn last_path_segment(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    let last = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .and_then(|last| {
            percent_encoding::percent_decode_str(last)
                .decode_utf8()
                .ok()
        })
        .map(|last| last.trim().to_string())
        .filter(|last| !last.is_empty());
    last.or_else(|| url.host_str().map(str::to_string))
}

/// 相手の応答を、ファイルの配信 (`download_content`) と同じ形の応答にして流す。
fn relay_response(
    upstream: ureq::http::Response<ureq::Body>,
    file_name: &str,
    permit: SemaphorePermit<'static>,
) -> Result<Response, AppError> {
    let status = upstream.status().as_u16();
    if !matches!(status, 200 | 206 | 416) {
        return Err(AppError::NotFound);
    }
    let status = StatusCode::from_u16(status).map_err(|_| AppError::NotFound)?;

    let mut headers = HeaderMap::new();
    for name in PASSED_HEADERS {
        if let Some(value) = upstream
            .headers()
            .get(name.as_str())
            .and_then(|value| HeaderValue::from_bytes(value.as_bytes()).ok())
        {
            headers.insert(name, value);
        }
    }
    // ADR: 返す型は相手の `Content-Type` をそのまま渡さず、こちらで決めたファイル名の拡張子から決める。
    // 相手が HTML を返したときに、このアプリのオリジンでスクリプトの動くページとして出さないため。
    // 名前は、URL の拡張子か、対象の種類だけから決めたもの (→ `file_name_from_response`)。
    // インラインで出すかの判定も、ファイルの配信と同じものを使う。
    let mime = mime_guess::from_path(file_name).first_or_octet_stream();
    let disposition = if contents::is_inline_allowed(&mime) {
        "inline"
    } else {
        "attachment"
    };
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.as_ref())
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        contents::content_disposition_header(disposition, file_name),
    );
    // 閲覧の権限を確かめて流すものなので、共有のキャッシュには置かせない。
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("private"));

    let (reader, writer) = tokio::io::duplex(RELAY_BUFFER_BYTES);
    let finished = Arc::new(AtomicBool::new(false));
    let finished_by_writer = Arc::clone(&finished);
    let mut body = upstream.into_body().into_reader();
    tokio::task::spawn_blocking(move || {
        // 受け取り終えるか、閲覧側が切るまで、中継の数に数える。
        let _permit = permit;
        let mut writer = SyncIoBridge::new(writer);
        match std::io::copy(&mut body, &mut writer) {
            // 終わった印を付けてから閉じる。読む側は閉じたのを見てから印を見る。
            Ok(_) => {
                finished_by_writer.store(true, Ordering::Release);
                drop(writer);
            }
            // 閲覧側が途中で切った (動画の早送りで頼み直したときなど) 場合もここに来る。
            Err(err) => tracing::debug!(%err, "stopped relaying a remote file"),
        }
    });

    let reader = CompletedReader::new(
        reader,
        finished,
        "the remote file was not relayed completely",
    );
    Ok((
        status,
        headers,
        Body::from_stream(ReaderStream::new(reader)),
    )
        .into_response())
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(remote_content))
        .routes(routes!(links_remote_content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relayed_file_name_takes_the_last_path_segment_of_files_with_a_known_kind() {
        assert_eq!(
            relayed_file_name("https://pdfobject.com/pdf/sample.pdf").as_deref(),
            Some("sample.pdf")
        );
        assert_eq!(
            relayed_file_name("https://example.com/v/clip.MP4?token=abc#t=10").as_deref(),
            Some("clip.MP4")
        );
        assert_eq!(
            relayed_file_name("http://example.com/photo.jpg").as_deref(),
            Some("photo.jpg")
        );
    }

    #[test]
    fn relayed_file_name_decodes_percent_encoded_names() {
        assert_eq!(
            relayed_file_name("https://example.com/%E8%B3%87%E6%96%99.pdf").as_deref(),
            Some("資料.pdf")
        );
    }

    #[test]
    fn file_name_from_response_adds_the_extension_of_the_content_type() {
        assert_eq!(
            file_name_from_response(
                "https://example.com/download",
                Some("application/PDF; charset=binary"),
                None,
                None
            )
            .as_deref(),
            Some("download.pdf")
        );
        // パスが空なら、ホスト名に足す。
        assert_eq!(
            file_name_from_response("https://example.com/", Some("video/mp4"), None, None)
                .as_deref(),
            Some("example.com.mp4")
        );
        // 名前に同じ拡張子があれば、重ねない。
        assert_eq!(
            file_name_from_response(
                "https://example.com/memo.TXT",
                Some("text/plain"),
                None,
                None
            )
            .as_deref(),
            Some("memo.TXT")
        );
    }

    #[test]
    fn file_name_from_response_prefers_the_disposition_and_the_redirected_url() {
        assert_eq!(
            file_name_from_response(
                "https://example.com/download?id=1",
                Some("application/octet-stream"),
                Some("attachment; filename=\"a.pdf\"; filename*=UTF-8''%E8%B3%87%E6%96%99.pdf"),
                None
            )
            .as_deref(),
            Some("資料.pdf")
        );
        assert_eq!(
            file_name_from_response(
                "https://example.com/download",
                Some("text/plain"),
                Some("attachment; filename=\"dir/notes.md\""),
                None
            )
            .as_deref(),
            Some("notes.md")
        );
        assert_eq!(
            file_name_from_response(
                "https://cdn.example.com/files/report.docx",
                Some("application/octet-stream"),
                None,
                None
            )
            .as_deref(),
            Some("report.docx")
        );
    }

    #[test]
    fn disposition_file_name_keeps_semicolons_and_escapes_inside_quotes() {
        assert_eq!(
            disposition_file_name(r#"attachment; filename="a;b \"c\".pdf"; size=10"#).as_deref(),
            Some(r#"a;b "c".pdf"#)
        );
        assert_eq!(
            disposition_file_name("inline; filename=report.pdf").as_deref(),
            Some("report.pdf")
        );
        assert_eq!(disposition_file_name("attachment"), None);
    }

    #[test]
    fn file_name_from_response_skips_pages_unknown_types_and_large_text() {
        for content_type in [
            "text/html; charset=utf-8",
            "image/svg+xml",
            "application/zip",
        ] {
            assert_eq!(
                file_name_from_response("https://example.com/x", Some(content_type), None, None),
                None,
                "{content_type}"
            );
        }
        assert_eq!(
            file_name_from_response("https://example.com/x", None, None, None),
            None
        );
        let too_large = super::super::text_files::TEXT_VIEWER_MAX_BYTES + 1;
        assert_eq!(
            file_name_from_response(
                "https://example.com/log",
                Some("text/plain"),
                None,
                Some(too_large)
            ),
            None
        );
        // テキストでなければ、大きくても開く。
        assert_eq!(
            file_name_from_response(
                "https://example.com/v",
                Some("video/webm"),
                None,
                Some(too_large)
            )
            .as_deref(),
            Some("v.webm")
        );
    }

    #[test]
    fn relayed_file_name_skips_pages_and_files_opened_in_a_new_tab() {
        assert_eq!(relayed_file_name("https://example.com/"), None);
        assert_eq!(relayed_file_name("https://example.com/docs/"), None);
        assert_eq!(relayed_file_name("https://example.com/index.html"), None);
        assert_eq!(relayed_file_name("https://example.com/archive.zip"), None);
        // パスでなくクエリにある拡張子では決めない。
        assert_eq!(
            relayed_file_name("https://example.com/view?file=a.pdf"),
            None
        );
        assert_eq!(relayed_file_name("ftp://example.com/a.pdf"), None);
    }
}
