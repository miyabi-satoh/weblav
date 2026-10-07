//! URL のファイルを中継し、ファイルと同じビューアで開けるようにする (→ docs/ui.md「URL のファイル」)。
//!
//! 中継するのは `link` コンテンツに登録された URL だけで、閲覧の権限を確かめてから取りに行く。
//! 誰でも任意の URL を取らせられる口にしないため。外へつなぐのは `link_title` の取得の口
//! (公開アドレスにだけつなぐ) に任せる。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use tokio_util::io::{ReaderStream, SyncIoBridge};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::completed_reader::CompletedReader;
use super::contents::{self, ContentType};
use super::link_title;
use crate::auth::Viewer;
use crate::error::{AppError, run_blocking};
use crate::state::AppState;

/// 相手から受け取って閲覧側へ送るまでの溜めの大きさ。閲覧側が読まない間 (動画の一時停止など) は
/// 相手からの受け取りも止まり、これより多くはメモリに載せない。
const RELAY_BUFFER_BYTES: usize = 256 * 1024;

/// 相手の応答から、そのまま閲覧側へ渡すヘッダー。Range と、ブラウザが取り直しを判断するもの。
/// `Content-Type` は渡さず、こちらで拡張子から決める (→ `relay_response`)。
const PASSED_HEADERS: [header::HeaderName; 5] = [
    header::CONTENT_LENGTH,
    header::CONTENT_RANGE,
    header::ACCEPT_RANGES,
    header::ETAG,
    header::LAST_MODIFIED,
];

/// `link` コンテンツの URL のファイルを、取りに行って流す。
/// 中継しない URL (拡張子で開き方が決まらない・リンクでない) と、相手が応答しない・エラーを返すときは 404。
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
    let file_name = relayed_file_name(&url).ok_or(AppError::NotFound)?;

    let range = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let upstream = run_blocking(move || link_title::open_file_blocking(&url, range.as_deref()))
        .await?
        .ok_or(AppError::NotFound)?;
    relay_response(upstream, &file_name)
}

/// URL を中継するなら、そのファイル名 (パスの最後の部分を戻したもの)。
/// 拡張子で開き方が決まるファイル (画面がビューア・プレイヤーで開くもの) だけを中継する。
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

/// 相手の応答を、ファイルの配信 (`download_content`) と同じ形の応答にして流す。
fn relay_response(
    upstream: ureq::http::Response<ureq::Body>,
    file_name: &str,
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
    // ADR: 型は相手の `Content-Type` でなく拡張子で決める。相手が HTML を返したときに、
    // このアプリのオリジンでスクリプトの動くページとして出さないため。インラインで出すかの判定も
    // ファイルの配信と同じものを使う。
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
    OpenApiRouter::new().routes(routes!(remote_content))
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
