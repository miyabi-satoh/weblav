//! リンクのカードに出すページの情報を、URL ごとに覚えて配る (→ docs/ui.md「リンクのカード」)。
//!
//! 一覧は覚えている情報ですぐに出し (`cached`)、画面がその後で取り直しを頼む (`refresh`)。

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use image::DynamicImage;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tokio::sync::Semaphore;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::link_title::{self, CacheHeaders, CardFetch, PageCard};
use super::thumbnails::{self, ThumbnailFormat};
use crate::auth::Viewer;
use crate::error::{AppError, AppJson, run_blocking};
use crate::state::AppState;

/// 覚えた画像とアイコンの全体の上限。超えたら、最後に一覧に出たのが古い順に消す。
/// ADR: 決め打ちにするのは、この上限と確かめる間隔の下限 (`MIN_RECHECK_SECS`) だけ。
/// 消しても、また一覧に出たときに取り直すだけなので、ディスクを使い切らない大きさにする。
const MAX_TOTAL_BYTES: i64 = 100 * 1024 * 1024;

/// 同じ URL を確かめに行く間隔の下限 (秒)。取れなかったときも含む。
/// 期限の指定が無いページや `no-cache` のページで、教室の端末が一斉に開いても相手を何度も突かないため。
const MIN_RECHECK_SECS: i64 = 5 * 60;

/// 1回の取り直しで受け付ける件数の上限。一覧に出す分を超える頼みは、壊れた画面か悪用とみなす。
const MAX_REFRESH_TARGETS: usize = 200;

/// 取りに行く画像の大きさの上限。カードの画像は縮めて使うので、大きな原寸は要らない。
const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;

/// カードの画像の長い辺 (px)。タイルのカードの幅の2倍を目安にする。
const CARD_IMAGE_SIZE: u32 = 640;

/// サイトのアイコンの一辺 (px)。小さく出すので、2倍の解像度の画面でも足りる大きさ。
const ICON_SIZE: u32 = 64;

/// 同時に外へ取りに行く数。教室の端末が一斉に開いても、相手のサイトと回線を突きすぎず、
/// デコードのメモリ (`thumbnails::DECODE_MAX_ALLOC` ずつ) も 1GB に収まる数。
const MAX_CONCURRENT_FETCHES: usize = 4;

/// 覚えた画像の置き場と、同時に外へ取りに行く数の上限。
pub struct LinkPreviews {
    dir: PathBuf,
    /// 一覧を開いた端末が一斉に取り直しを頼んでも、外への接続とデコードを並べすぎないよう絞る。
    permits: Semaphore,
}

impl LinkPreviews {
    /// `dir` の作成は最初に画像を置くときまで遅らせる (縮小画像と同じ)。
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            permits: Semaphore::new(MAX_CONCURRENT_FETCHES),
        }
    }
}

/// リンクのカードに出す情報。URL はこのサーバーから配る画像の URL。
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkPreview {
    pub title: Option<String>,
    pub site_name: Option<String>,
    /// ページの `article:published_time` のまま。読めなければ経過を出さない。
    pub published_at: Option<String>,
    pub image_url: Option<String>,
    pub icon_url: Option<String>,
}

struct PreviewRow {
    url: String,
    title: Option<String>,
    site_name: Option<String>,
    published_at: Option<String>,
    image_file: Option<String>,
    icon_file: Option<String>,
}

impl From<PreviewRow> for LinkPreview {
    fn from(row: PreviewRow) -> Self {
        Self {
            title: row.title,
            site_name: row.site_name,
            published_at: row.published_at,
            image_url: row.image_file.map(|file| file_url(&file)),
            icon_url: row.icon_file.map(|file| file_url(&file)),
        }
    }
}

fn file_url(file: &str) -> String {
    format!("/api/v1/link-previews/files/{file}")
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

/// 覚えている情報を返す。取りには行かない。まだ覚えていない URL は入らない。
pub(super) async fn cached(
    pool: &SqlitePool,
    urls: &[String],
) -> Result<HashMap<String, LinkPreview>, AppError> {
    if urls.is_empty() {
        return Ok(HashMap::new());
    }
    let urls_json = serde_json::to_string(urls).expect("文字列の並びは JSON にできる");
    let rows = sqlx::query_as!(
        PreviewRow,
        r#"SELECT url, title, site_name, published_at, image_file, icon_file
           FROM link_previews WHERE url IN (SELECT value FROM json_each(?))"#,
        urls_json
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.url.clone(), LinkPreview::from(row)))
        .collect())
}

/// 取り直しの対象。見る権限は呼び出し側が確かめる。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct RefreshLinkPreviewsRequest {
    /// `link` コンテンツの id。見られないもの・リンクでないものは黙って飛ばす。
    content_ids: Vec<i64>,
    /// リンクの一覧のファイル。中のリンクを取り直す。見られないもの・一覧のファイルでないものは黙って飛ばす。
    links_file: Option<RefreshLinksFile>,
}

/// 取り直す一覧のファイル。指し方は `GET /contents/{id}/links` と同じ。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct RefreshLinksFile {
    content_id: i64,
    #[serde(flatten)]
    query: super::links_file::LinksFileQuery,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct RefreshedLinkPreview {
    url: String,
    preview: LinkPreview,
}

/// 一覧に出したリンクのページの情報を、期限が切れていれば取り直して返す。
/// 一覧は覚えている情報ですぐ出し、これの答えで差し替える (→ docs/ui.md「リンクのカード」)。
#[utoipa::path(
    post,
    path = "/link-previews/refresh",
    request_body = RefreshLinkPreviewsRequest,
    responses(
        (status = OK, body = Vec<RefreshedLinkPreview>, description = "頼まれたリンクの今の情報。取れなかったものも、覚えている情報で返す"),
        (status = 422, body = crate::error::ErrorResponse, description = "件数が多すぎる"),
    )
)]
async fn refresh_link_previews(
    viewer: Viewer,
    State(state): State<AppState>,
    AppJson(payload): AppJson<RefreshLinkPreviewsRequest>,
) -> Result<Json<Vec<RefreshedLinkPreview>>, AppError> {
    if payload.content_ids.len() > MAX_REFRESH_TARGETS {
        return Err(AppError::Validation(
            "too many links to refresh".to_string(),
        ));
    }
    let mut urls = Vec::new();
    for id in payload.content_ids {
        if let Some(url) = super::contents::viewable_link_url(&state.pool, &viewer, id).await? {
            urls.push(url);
        }
    }
    if let Some(target) = payload.links_file {
        match super::links_file::viewable_urls(&state, &viewer, target.content_id, &target.query)
            .await
        {
            // 一覧のファイルは手やジョブが書くので、上限を超えた分は断らずに取り直さないだけにする。
            Ok(file_urls) => {
                let room = MAX_REFRESH_TARGETS.saturating_sub(urls.len());
                urls.extend(file_urls.into_iter().take(room));
            }
            Err(AppError::NotFound | AppError::Unauthorized) => {}
            Err(err) => return Err(err),
        }
    }
    let previews = refresh(&state, urls).await?;
    Ok(Json(
        previews
            .into_iter()
            .map(|(url, preview)| RefreshedLinkPreview { url, preview })
            .collect(),
    ))
}

/// `urls` の情報を、期限が切れていれば取り直して返す。どれも一覧に出たものとして覚える。
pub(super) async fn refresh(
    state: &AppState,
    urls: Vec<String>,
) -> Result<Vec<(String, LinkPreview)>, AppError> {
    let mut seen = HashSet::new();
    let urls: Vec<String> = urls
        .into_iter()
        .filter(|url| seen.insert(url.clone()))
        .collect();
    let now = now_secs();
    // 一覧に出た時刻をまとめて1文で書く。匿名でも何度でも頼めるので、件数分の書き込みにしない。
    let urls_json = serde_json::to_string(&urls).expect("文字列の並びは JSON にできる");
    sqlx::query!(
        r#"INSERT INTO link_previews (url, shown_at) SELECT value, ? FROM json_each(?) WHERE true
           ON CONFLICT (url) DO UPDATE SET shown_at = excluded.shown_at"#,
        now,
        urls_json
    )
    .execute(&state.pool)
    .await?;

    let mut tasks = tokio::task::JoinSet::new();
    for url in &urls {
        let (state, url) = (state.clone(), url.clone());
        tasks.spawn(async move { refresh_one(&state, &url, now).await });
    }
    while let Some(result) = tasks.join_next().await {
        // 1件の失敗 (DB の異常など) で、ほかのリンクの答えまで捨てない。
        match result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => tracing::warn!(error = %err, "failed to refresh a link preview"),
            Err(err) => tracing::warn!(error = %err, "link preview task failed"),
        }
    }
    enforce_total_limit(&state.pool, &state.link_previews.dir, MAX_TOTAL_BYTES).await?;

    let mut previews = cached(&state.pool, &urls).await?;
    Ok(urls
        .into_iter()
        .map(|url| {
            let preview = previews.remove(&url).unwrap_or_default();
            (url, preview)
        })
        .collect())
}

/// 1件を、期限が切れていて、前回確かめてから間があいていれば取り直す。
async fn refresh_one(state: &AppState, url: &str, now: i64) -> Result<(), AppError> {
    let recheck_before = now - MIN_RECHECK_SECS;
    // 確かめに行く権利を先に取る。同じ URL を同時に頼まれても、取りに行くのは1つだけにするため。
    let claimed = sqlx::query!(
        r#"UPDATE link_previews SET checked_at = ?
           WHERE url = ? AND fresh_until <= ? AND checked_at <= ?
           RETURNING etag, last_modified, image_source, image_file, icon_source, icon_file"#,
        now,
        url,
        now,
        recheck_before
    )
    .fetch_optional(&state.pool)
    .await?;
    let Some(claimed) = claimed else {
        return Ok(());
    };

    let _permit = state
        .link_previews
        .permits
        .acquire()
        .await
        .expect("閉じないセマフォ");
    let fetch = {
        let url = url.to_string();
        let (etag, last_modified) = (claimed.etag.clone(), claimed.last_modified.clone());
        run_blocking(move || {
            link_title::fetch_card_blocking(&url, etag.as_deref(), last_modified.as_deref())
        })
        .await?
    };
    let old = Slots {
        image: Slot {
            source: claimed.image_source,
            file: claimed.image_file,
        },
        icon: Slot {
            source: claimed.icon_source,
            file: claimed.icon_file,
        },
    };
    let (card, headers, sources) = match fetch {
        CardFetch::Failed => return Ok(()),
        CardFetch::NotModified(mut headers) => {
            // 304 の応答は検証子を付けないことがある。覚えている応答のヘッダーで補う (RFC 9111 4.3.4)。
            headers.etag = headers.etag.or(claimed.etag);
            headers.last_modified = headers.last_modified.or(claimed.last_modified);
            let sources = (old.image.source.clone(), old.icon.source.clone());
            (None, headers, sources)
        }
        CardFetch::Fetched { card, headers } => {
            let sources = (card.image_url.clone(), card.icon_url.clone());
            (Some(card), headers, sources)
        }
    };
    let settled = settle_images(&state.link_previews.dir, &old, sources).await?;
    save(state, url, card.as_ref(), &headers, now, &old, settled).await
}

/// 画像かアイコンの、元の URL と置いたファイル。
struct Slot {
    source: Option<String>,
    file: Option<String>,
}

struct Slots {
    image: Slot,
    icon: Slot,
}

/// 取り直した後の画像とアイコン。バイト数は全体の上限に数える。
struct Settled {
    image: Slot,
    image_bytes: i64,
    icon: Slot,
    icon_bytes: i64,
}

/// ページが指す画像とアイコンを置き場にそろえる。元が変わったときに加えて、前に取れなかった・
/// 置いたファイルが無くなったときも取り直す。一度の失敗や、上限で消し合ったファイルのせいで、
/// 画像の無いカードのまま固まらないため。
async fn settle_images(
    dir: &Path,
    old: &Slots,
    sources: (Option<String>, Option<String>),
) -> Result<Settled, AppError> {
    let dir = dir.to_path_buf();
    let old = [
        (old.image.source.clone(), old.image.file.clone()),
        (old.icon.source.clone(), old.icon.file.clone()),
    ];
    run_blocking(move || {
        let settle = |source: Option<String>,
                      (old_source, old_file): (Option<String>, Option<String>),
                      kind| {
            let Some(source) = source else {
                return Slot {
                    source: None,
                    file: None,
                };
            };
            let kept = old_file.filter(|file| {
                old_source.as_deref() == Some(source.as_str()) && dir.join(file).is_file()
            });
            let file = kept.or_else(|| {
                link_title::fetch_image_blocking(&source, MAX_IMAGE_BYTES)
                    .and_then(|bytes| shrink(&bytes, kind))
                    .and_then(|(bytes, format)| store_file(&dir, &bytes, format).ok())
            });
            Slot {
                source: Some(source),
                file,
            }
        };
        let [old_image, old_icon] = old;
        let image = settle(sources.0, old_image, ImageKind::Card);
        let icon = settle(sources.1, old_icon, ImageKind::Icon);
        let size = |slot: &Slot| {
            slot.file
                .as_ref()
                .and_then(|file| std::fs::metadata(dir.join(file)).ok())
                .map_or(0, |meta| i64::try_from(meta.len()).unwrap_or(i64::MAX))
        };
        Settled {
            image_bytes: size(&image),
            image,
            icon_bytes: size(&icon),
            icon,
        }
    })
    .await
}

/// 取り直した結果を書く。`card` が `None` (304) なら、ページの情報は前のままにする。
async fn save(
    state: &AppState,
    url: &str,
    card: Option<&PageCard>,
    headers: &CacheHeaders,
    now: i64,
    old: &Slots,
    mut settled: Settled,
) -> Result<(), AppError> {
    let mut fresh_until = fresh_until(now, headers);
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let exists = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM link_previews WHERE url = ?) as "exists!: bool""#,
        url
    )
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        // 取りに行っている間に、上限を超えて消された。置いたファイルを残さず、また一覧に出たときに取り直す。
        drop(tx);
        let files = [settled.image.file, settled.icon.file]
            .into_iter()
            .flatten()
            .collect();
        return remove_unreferenced(&state.pool, &state.link_previews.dir, files).await;
    }
    // 消す側 (`remove_unreferenced`) と同じ書き込みのロックの中で確かめるので、確かめた後には消されない。
    // 消されていたら、間をおいてまた取り直させる。
    if drop_missing_files(&state.link_previews.dir, &mut settled).await? {
        fresh_until = now;
    }
    let new_files = [&settled.image.file, &settled.icon.file];
    let bytes = settled.image_bytes + settled.icon_bytes;
    if let Some(card) = card {
        sqlx::query!(
            "UPDATE link_previews SET title = ?, site_name = ?, published_at = ? WHERE url = ?",
            card.title,
            card.site_name,
            card.published_at,
            url
        )
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query!(
        r#"UPDATE link_previews SET
               image_source = ?, image_file = ?, icon_source = ?, icon_file = ?,
               etag = ?, last_modified = ?, fresh_until = ?, bytes = ?
           WHERE url = ?"#,
        settled.image.source,
        settled.image.file,
        settled.icon.source,
        settled.icon.file,
        headers.etag,
        headers.last_modified,
        fresh_until,
        bytes,
        url
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let kept: Vec<&String> = new_files.into_iter().flatten().collect();
    let replaced = [&old.image.file, &old.icon.file]
        .into_iter()
        .flatten()
        .filter(|file| !kept.contains(file))
        .cloned()
        .collect();
    remove_unreferenced(&state.pool, &state.link_previews.dir, replaced).await
}

/// 置いた後で消されたファイルを無しにする。ほかの取り直しが、まだどの行も指していないと見て消していることがある。
/// 無くなったものがあれば `true`。
async fn drop_missing_files(dir: &Path, settled: &mut Settled) -> Result<bool, AppError> {
    let mut missing = false;
    for (slot, bytes) in [
        (&mut settled.image, &mut settled.image_bytes),
        (&mut settled.icon, &mut settled.icon_bytes),
    ] {
        if let Some(file) = &slot.file
            && !tokio::fs::try_exists(dir.join(file)).await?
        {
            slot.file = None;
            *bytes = 0;
            missing = true;
        }
    }
    Ok(missing)
}

/// どの行からも指されなくなったファイルを消す。アイコンは同じサイトのページで共有するため、確かめてから消す。
/// 書き込みのロックを持ったまま確かめて消す。`save` が同じロックの中でファイルの有無を確かめるので、
/// 確かめた後に別の行が同じファイルを指すことはない。
async fn remove_unreferenced(
    pool: &SqlitePool,
    dir: &Path,
    files: Vec<String>,
) -> Result<(), AppError> {
    if files.is_empty() {
        return Ok(());
    }
    let mut tx = crate::db::begin_write(pool).await?;
    for file in files {
        let referenced = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM link_previews WHERE image_file = ?1 OR icon_file = ?1) as "referenced!: bool""#,
            file
        )
        .fetch_one(&mut *tx)
        .await?;
        if !referenced {
            let path = dir.join(&file);
            if let Err(err) = tokio::fs::remove_file(&path).await
                && err.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(error = %err, file, "failed to remove a link preview image");
            }
        }
    }
    tx.commit().await?;
    Ok(())
}

/// 全体が `max_bytes` を超えていたら、最後に一覧に出たのが古い順に消す。
/// 消す行は書き込みのロックの中で数え直して選ぶ。間に一覧に出た行を、古いものとして消さないため。
/// 超えていないときはロックを取らない。取り直しのたびに呼ばれるので、ほかの書き込みを待たせない。
async fn enforce_total_limit(
    pool: &SqlitePool,
    dir: &Path,
    max_bytes: i64,
) -> Result<(), AppError> {
    if total_bytes(pool).await? <= max_bytes {
        return Ok(());
    }
    let mut tx = crate::db::begin_write(pool).await?;
    let total = total_bytes(&mut *tx).await?;
    if total <= max_bytes {
        return Ok(());
    }
    let rows = sqlx::query!(
        r#"SELECT url, image_file, icon_file, bytes FROM link_previews ORDER BY shown_at, url"#
    )
    .fetch_all(&mut *tx)
    .await?;
    let mut over = total - max_bytes;
    let mut files = Vec::new();
    for row in rows {
        if over <= 0 {
            break;
        }
        sqlx::query!("DELETE FROM link_previews WHERE url = ?", row.url)
            .execute(&mut *tx)
            .await?;
        over -= row.bytes;
        files.extend(row.image_file);
        files.extend(row.icon_file);
    }
    tx.commit().await?;
    remove_unreferenced(pool, dir, files).await
}

async fn total_bytes(
    executor: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
) -> Result<i64, AppError> {
    Ok(
        sqlx::query_scalar!(
            r#"SELECT COALESCE(SUM(bytes), 0) as "total!: i64" FROM link_previews"#
        )
        .fetch_one(executor)
        .await?,
    )
}

/// 次に確かめに行くまで、覚えた情報をそのまま使ってよい時刻 (RFC 9111 4.2)。
/// 共有のキャッシュではないので `s-maxage` と `private` は見ない。`no-store` も、表示に使う
/// 情報を手元に置くしかないので `no-cache` と同じく「毎回確かめる」として扱う。
fn fresh_until(now: i64, headers: &CacheHeaders) -> i64 {
    let directives = CacheControl::parse(headers.cache_control.as_deref());
    if directives.no_cache {
        return now;
    }
    let date = headers.date.as_deref().and_then(http_date).unwrap_or(now);
    let lifetime = directives
        .max_age
        .or_else(|| {
            let expires = headers.expires.as_deref()?;
            // 読めない `Expires` は、もう期限切れとみなす (RFC 9111 5.3)。
            Some(http_date(expires).map_or(0, |expires| expires - date))
        })
        .or_else(|| {
            // 発見的な期限 (RFC 9111 4.2.2)。`Last-Modified` からの経過の1割にする。
            let last_modified = http_date(headers.last_modified.as_deref()?)?;
            Some((date - last_modified) / 10)
        })
        .unwrap_or(0)
        .max(0);
    // 応答がどれだけ古いか (RFC 9111 4.2.3)。中継のキャッシュの `Age` と、時計のずれを見る。
    let age = headers
        .age
        .as_deref()
        .and_then(|age| age.trim().parse::<i64>().ok())
        .unwrap_or(0);
    let current_age = (now - date).max(age).max(0);
    now + (lifetime - current_age).max(0)
}

fn http_date(value: &str) -> Option<i64> {
    let time = httpdate::parse_http_date(value.trim()).ok()?;
    let secs = time.duration_since(UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(secs).ok()
}

#[derive(Debug, Default, PartialEq, Eq)]
struct CacheControl {
    no_cache: bool,
    max_age: Option<i64>,
}

impl CacheControl {
    fn parse(value: Option<&str>) -> Self {
        let mut parsed = Self::default();
        for directive in value.unwrap_or_default().split(',') {
            let (name, argument) = match directive.split_once('=') {
                Some((name, argument)) => (name, Some(argument.trim().trim_matches('"'))),
                None => (directive, None),
            };
            match name.trim().to_ascii_lowercase().as_str() {
                "no-cache" | "no-store" => parsed.no_cache = true,
                "max-age" => {
                    // 読めない `max-age` は、期限切れとみなす (RFC 9111 4.2.1)。
                    parsed.max_age =
                        Some(argument.and_then(|value| value.parse().ok()).unwrap_or(0));
                }
                _ => {}
            }
        }
        parsed
    }
}

#[derive(Clone, Copy)]
enum ImageKind {
    Card,
    Icon,
}

/// 取った画像を縮める。読めない画像は `None`。アイコンは透過を残すため PNG にし、
/// カードの画像は透過が無ければ JPEG にする。
fn shrink(bytes: &[u8], kind: ImageKind) -> Option<(Vec<u8>, ThumbnailFormat)> {
    let image = thumbnails::decode(Cursor::new(bytes))?;
    let image = match kind {
        ImageKind::Card => fit_within(image, CARD_IMAGE_SIZE),
        ImageKind::Icon => fit_within(image, ICON_SIZE),
    };
    thumbnails::encode(&image, matches!(kind, ImageKind::Icon))
}

/// 長い辺が `size` に収まるよう縮める。小さい画像は拡大しない。
fn fit_within(image: DynamicImage, size: u32) -> DynamicImage {
    if image.width() <= size && image.height() <= size {
        image
    } else {
        image.thumbnail(size, size)
    }
}

/// 中身のハッシュを名前にして置く。同じアイコンを使うページが多いので、1つの実体を共有する。
fn store_file(dir: &Path, bytes: &[u8], format: ThumbnailFormat) -> Result<String, AppError> {
    let name = format!(
        "{}.{}",
        super::hex_encode(&Sha256::digest(bytes)),
        format.extension()
    );
    let dest = dir.join(&name);
    if !dest.is_file() {
        thumbnails::store(&dest, &name, bytes)?;
    }
    Ok(name)
}

/// 覚えた画像を配る。名前は中身のハッシュなので、ブラウザには取り直させない。
#[utoipa::path(
    get,
    path = "/link-previews/files/{name}",
    params(("name" = String, Path, description = "画像のファイル名 (中身のハッシュと拡張子)")),
    responses(
        (status = OK, description = "画像", content_type = "image/*"),
        (status = 404, body = crate::error::ErrorResponse, description = "覚えていない (上限を超えて消されたなど)"),
    )
)]
async fn link_preview_file(
    State(state): State<AppState>,
    UrlPath(name): UrlPath<String>,
) -> Result<Response, AppError> {
    // 名前を中身のハッシュと知っている拡張子の形に限り、置き場の外を指させない。
    let format = name
        .rsplit_once('.')
        .filter(|(hash, _)| super::contents::is_valid_blob_hash(hash))
        .and_then(|(_, extension)| {
            ThumbnailFormat::ALL
                .into_iter()
                .find(|format| format.extension() == extension)
        })
        .ok_or(AppError::NotFound)?;
    let path = state.link_previews.dir.join(&name);
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err(AppError::NotFound),
        Err(err) => return Err(err.into()),
    };
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static(format.mime()),
            ),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            ),
        ],
        bytes,
    )
        .into_response())
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(refresh_link_previews))
        .routes(routes!(link_preview_file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};

    const NOW: i64 = 1_800_000_000;

    fn headers(pairs: &[(&str, &str)]) -> CacheHeaders {
        let get = |name: &str| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
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

    fn http(secs: i64) -> String {
        httpdate::fmt_http_date(UNIX_EPOCH + std::time::Duration::from_secs(secs as u64))
    }

    #[test]
    fn fresh_until_follows_max_age_minus_the_age_of_the_response() {
        let date = http(NOW);
        assert_eq!(
            fresh_until(
                NOW,
                &headers(&[("cache-control", "public, max-age=600"), ("date", &date)])
            ),
            NOW + 600
        );
        assert_eq!(
            fresh_until(
                NOW,
                &headers(&[
                    ("cache-control", "max-age=\"600\""),
                    ("age", "100"),
                    ("date", &date)
                ])
            ),
            NOW + 500
        );
    }

    #[test]
    fn fresh_until_is_now_when_the_page_asks_to_be_checked_every_time() {
        for value in [
            "no-cache",
            "no-store",
            "max-age=600, no-cache",
            "max-age=abc",
        ] {
            assert_eq!(
                fresh_until(NOW, &headers(&[("cache-control", value)])),
                NOW,
                "{value}"
            );
        }
    }

    #[test]
    fn fresh_until_uses_expires_relative_to_the_date_header() {
        let (date, expires) = (http(NOW - 60), http(NOW + 240));
        assert_eq!(
            fresh_until(NOW, &headers(&[("date", &date), ("expires", &expires)])),
            NOW + 240
        );
        assert_eq!(
            fresh_until(NOW, &headers(&[("expires", "0")])),
            NOW,
            "読めない Expires は期限切れ"
        );
    }

    /// 期限の指定が無ければ、`Last-Modified` からの経過の1割 (RFC 9111 4.2.2)。どちらも無ければ毎回確かめる。
    #[test]
    fn fresh_until_falls_back_to_a_tenth_of_the_time_since_last_modified() {
        let (date, last_modified) = (http(NOW), http(NOW - 10_000));
        assert_eq!(
            fresh_until(
                NOW,
                &headers(&[("date", &date), ("last-modified", &last_modified)])
            ),
            NOW + 1_000
        );
        assert_eq!(fresh_until(NOW, &headers(&[])), NOW);
    }

    #[test]
    fn shrink_fits_card_images_and_keeps_icons_as_png() {
        let mut wide = Vec::new();
        DynamicImage::ImageRgb8(RgbImage::from_pixel(1600, 800, Rgb([10, 20, 30])))
            .write_to(&mut Cursor::new(&mut wide), ImageFormat::Png)
            .expect("PNG を作れるはず");
        let (bytes, format) = shrink(&wide, ImageKind::Card).expect("縮められるはず");
        assert!(
            matches!(format, ThumbnailFormat::Jpeg),
            "透過が無ければ JPEG"
        );
        let card = thumbnails::decode(Cursor::new(&bytes)).expect("読めるはず");
        assert_eq!(
            (card.width(), card.height()),
            (CARD_IMAGE_SIZE, CARD_IMAGE_SIZE / 2)
        );

        let mut ico = Vec::new();
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(128, 128, Rgba([0, 0, 0, 0])))
            .write_to(&mut Cursor::new(&mut ico), ImageFormat::Ico)
            .expect("ICO を作れるはず");
        let (bytes, format) = shrink(&ico, ImageKind::Icon).expect("favicon.ico を読めるはず");
        assert!(matches!(format, ThumbnailFormat::Png));
        let icon = thumbnails::decode(Cursor::new(&bytes)).expect("読めるはず");
        assert_eq!((icon.width(), icon.height()), (ICON_SIZE, ICON_SIZE));

        assert!(shrink(b"<html></html>", ImageKind::Card).is_none());
    }

    /// 元が同じで置いたファイルが残っていれば使い回し、無くなっていれば取り直す (ここでは外へつながらず取れない)。
    #[tokio::test]
    async fn settle_images_refetches_when_the_stored_file_is_gone() {
        let dir = crate::test_support::project_temp_dir("link_preview", "settle");
        let kept = format!("{}.png", "b".repeat(64));
        std::fs::write(dir.join(&kept), b"png").expect("書けるはず");
        let source = Some("http://127.0.0.1/icon.png".to_string());
        let old = Slots {
            image: Slot {
                source: source.clone(),
                file: Some(format!("{}.jpg", "a".repeat(64))),
            },
            icon: Slot {
                source: source.clone(),
                file: Some(kept.clone()),
            },
        };

        let settled = settle_images(&dir, &old, (source.clone(), source))
            .await
            .expect("揃えられるはず");

        assert_eq!(settled.icon.file.as_deref(), Some(kept.as_str()));
        assert_eq!(
            settled.image.file, None,
            "無くなった画像は取り直し、取れなければ無しにする"
        );
        assert_eq!((settled.image_bytes, settled.icon_bytes), (0, 3));
    }

    #[tokio::test]
    async fn drop_missing_files_clears_only_the_files_removed_after_storing() {
        let dir = crate::test_support::project_temp_dir("link_preview", "missing");
        let kept = format!("{}.png", "b".repeat(64));
        std::fs::write(dir.join(&kept), b"png").expect("書けるはず");
        let mut settled = Settled {
            image: Slot {
                source: Some("http://127.0.0.1/image.jpg".to_string()),
                file: Some(format!("{}.jpg", "a".repeat(64))),
            },
            image_bytes: 10,
            icon: Slot {
                source: Some("http://127.0.0.1/icon.png".to_string()),
                file: Some(kept.clone()),
            },
            icon_bytes: 3,
        };

        let missing = drop_missing_files(&dir, &mut settled)
            .await
            .expect("確かめられるはず");

        assert!(missing);
        assert_eq!(settled.image.file, None, "消されたファイルは指さない");
        assert_eq!(
            settled.image.source.as_deref(),
            Some("http://127.0.0.1/image.jpg"),
            "元の URL は残し、次に取り直す"
        );
        assert_eq!(settled.icon.file.as_deref(), Some(kept.as_str()));
        assert_eq!((settled.image_bytes, settled.icon_bytes), (0, 3));
    }

    async fn insert(pool: &SqlitePool, url: &str, shown_at: i64, bytes: i64, files: (&str, &str)) {
        sqlx::query!(
            "INSERT INTO link_previews (url, shown_at, bytes, image_file, icon_file) VALUES (?, ?, ?, ?, ?)",
            url,
            shown_at,
            bytes,
            files.0,
            files.1
        )
        .execute(pool)
        .await
        .expect("入れられるはず");
    }

    /// 上限を超えたら一覧に出たのが古い順に消し、ほかの行が使うアイコンは残す。
    #[sqlx::test]
    async fn enforce_total_limit_drops_the_least_recently_shown(pool: SqlitePool) {
        let dir = crate::test_support::project_temp_dir("link_preview", "limit");
        for name in ["old.jpg", "new.jpg", "icon.png"] {
            std::fs::write(dir.join(name), b"x").expect("書けるはず");
        }
        insert(
            &pool,
            "https://a.example/old",
            1,
            60,
            ("old.jpg", "icon.png"),
        )
        .await;
        insert(
            &pool,
            "https://a.example/new",
            2,
            60,
            ("new.jpg", "icon.png"),
        )
        .await;

        enforce_total_limit(&pool, &dir, 100)
            .await
            .expect("消せるはず");

        let left = cached(
            &pool,
            &[
                "https://a.example/old".to_string(),
                "https://a.example/new".to_string(),
            ],
        )
        .await
        .expect("読めるはず");
        assert_eq!(left.keys().collect::<Vec<_>>(), ["https://a.example/new"]);
        assert!(!dir.join("old.jpg").exists());
        assert!(
            dir.join("icon.png").exists(),
            "残る行が使うアイコンは消さない"
        );
        assert!(dir.join("new.jpg").exists());
    }
}
