//! アーカイブのアイテム一覧・公開切り替え・配信 (→ docs/archive.md「アイテムの公開」・「アイテムの配信」・「エンドポイント一覧」)。
//!
//! 属性値・表示タイトルの導出そのものは `archive` モジュールに、導出済みの全アイテムを持つ所と
//! 絞り込み・並べ替えは `archive_index` に置き、ここでは区切った結果をレスポンスの形に組み立てる
//! (→ docs/archive.md「大きなアーカイブの一覧」)。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::{AuthUser, Viewer};
use crate::error::{AppError, AppJson, run_blocking};
use crate::folder_reach::OTHERS_WAIT;
use crate::state::AppState;

use super::archive::{self, ArchiveMeta, ValueKey};
use super::archive_index::{
    self, ArchiveIndex, IndexedItem, ListQuery, Snapshot, SnapshotKind, StatusCounts, StatusFilter,
    StoredPreview,
};
use super::contents;
use super::contents::FolderEntry;
use super::folder_marks::Marks;
use super::folder_search::{ListedTitle, SearchFileHit};
use super::search::FieldKind;
use super::sort::title_cmp;
use super::thumbnails;

/// 1回に返す行の数の既定と上限。行は約200バイトなので1回100KB 程度になり、
/// スマートフォンでも待たせず、スクロールで次を取る回数も多すぎない (→ docs/archive.md「一覧の API を区切って返す」)。
const PAGE_LIMIT: usize = 500;

/// 区切りの引数と画面の中の絞り込みのほかに、軸の絞り込みに付ける頭。軸の名前をそのままキーにすると、
/// `offset`・`status` などの名前の軸と取り違えるため (→ docs/archive.md「一覧の API を区切って返す」)。
const AXIS_QUERY_PREFIX: &str = "axis.";

/// 閲覧用エンドポイント向けに、公開範囲の判定込みでアーカイブを取得する。
/// パンくずに使えるよう、判定で辿った祖先も返す。
/// ファイルシステムに触る前に判定する理由は `contents::folder_root` と同じ。
pub(super) async fn load_viewable_archive(
    pool: &sqlx::SqlitePool,
    viewer: &Viewer,
    id: i64,
) -> Result<(ArchiveMeta, contents::Lineage), AppError> {
    let meta = archive::load_archive(pool, id).await?;
    let lineage = contents::ensure_viewable(pool, viewer, id).await?;
    Ok((meta, lineage))
}

/// アーカイブのアイテムの数と、そのうち公開しているものの数。
/// 公開し忘れ・公開しすぎに気づけるよう、管理画面の一覧と編集ページの見出しに出す (→ docs/ui.md「UI 全般」)。
#[derive(Debug, Clone, Copy, Default, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveItemCounts {
    pub total: i64,
    pub published: i64,
}

/// 一覧の表示を使うフォルダーごとのアイテムの数。アイテムが1件も無いものは入らない。
/// 印はメモリに持ったものを使う。全部を公開に切り替えたフォルダーでは印が件数ぶんあり、開くたびに読み直すと重いため。
pub(super) async fn item_counts(
    state: &AppState,
) -> Result<HashMap<i64, ArchiveItemCounts>, AppError> {
    let ids =
        sqlx::query_scalar!(r#"SELECT DISTINCT archive_id as "archive_id!" FROM archive_items"#)
            .fetch_all(&state.pool)
            .await?;
    let mut counts = HashMap::with_capacity(ids.len());
    for id in ids {
        let (_, marks) = archive_index::marks(state, id).await?;
        counts.insert(id, count_items(&state.pool, id, marks).await?);
    }
    Ok(counts)
}

/// 1つのフォルダーのアイテムの数。書き込みの応答に添えるもので、呼ばれるのは1件ずつなので印は読み直す。
pub(super) async fn item_counts_of(
    pool: &sqlx::SqlitePool,
    archive_id: i64,
) -> Result<ArchiveItemCounts, AppError> {
    let marks = Arc::new(Marks::load(pool, archive_id).await?);
    count_items(pool, archive_id, marks).await
}

async fn count_items(
    pool: &sqlx::SqlitePool,
    archive_id: i64,
    marks: Arc<Marks>,
) -> Result<ArchiveItemCounts, AppError> {
    let rel_paths = sqlx::query_scalar!(
        "SELECT rel_path FROM archive_items WHERE archive_id = ?",
        archive_id
    )
    .fetch_all(pool)
    .await?;
    run_blocking(move || {
        let published = rel_paths.iter().filter(|rel| marks.is_public(rel)).count();
        ArchiveItemCounts {
            total: i64::try_from(rel_paths.len()).unwrap_or(i64::MAX),
            published: i64::try_from(published).unwrap_or(i64::MAX),
        }
    })
    .await
}

// --- GET /contents/{id}/archive (閲覧用) ---

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ArchiveAxisOption {
    /// 表示名。表示名が同じ値は1つの選択肢にまとめる (→ docs/archive.md「軸の値の辞書と導出」)。
    /// 絞り込みのクエリにもこの値をそのまま載せる (→ docs/archive.md「エンドポイント一覧」)。
    value: String,
    /// **この選択肢を選んだときに残る件数** (→ docs/archive.md「エンドポイント一覧」)。
    /// 他の軸・名前の語・公開の状態の絞り込みは効かせたまま、この軸だけをこの値に置き換えて数える。
    /// 0件になる組み合わせを選ぶ前に見分けられるようにするためのもので、
    /// 選択肢自体は 0 でも消さない (緩める側の選択肢を消さない方針は変えない)。
    count: usize,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ArchiveAxisResponse {
    /// 絞り込みに出す軸のうち、選べる値があるものだけを返す (→ docs/archive.md「軸の定義」・「軸の値の辞書と導出」)。
    name: String,
    /// **絞り込み前の**、閲覧者に見えるアイテム全体から作る (→ docs/archive.md「軸の値の辞書と導出」)。
    /// 選択肢を絞り込み条件では狭めない (緩める側の選択肢を消さないため)。
    options: Vec<ArchiveAxisOption>,
}

fn axes_response(axes: Vec<archive_index::AxisOptions>) -> Vec<ArchiveAxisResponse> {
    axes.into_iter()
        .map(|axis| ArchiveAxisResponse {
            name: axis.name,
            options: axis
                .options
                .into_iter()
                .map(|(value, count)| ArchiveAxisOption { value, count })
                .collect(),
        })
        .collect()
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ArchiveViewItem {
    id: i64,
    title: String,
    /// `rel_path` の最後の要素。ページ内プレイヤーへの振り分け (→ docs/ui.md「音声のページ内プレイヤー」) に使う。
    file_name: String,
    /// アーカイブの登録先からの相対パス。Markdown のプレビューが、相対パスの画像とリンクを
    /// このファイルの場所から解決するのに使う (→ docs/ui.md「PDF・動画・テキストのビューアー」)。
    rel_path: String,
    /// 行の2段目に出す軸の値 (→ docs/ui.md「アーカイブの一覧画面」)。出す値が無ければ `None`。
    subtitle: Option<String>,
    /// ページ内でプレビューする画像なら、その大きさ (→ docs/ui.md「画像のプレビュー」)。
    image: Option<thumbnails::ImageSize>,
    /// テキストのビューアーで見せるファイルか (→ docs/ui.md「PDF・動画・テキストのビューアー」)。
    is_text: bool,
    /// 行に縮小画像を出してみるか (→ docs/ui.md「画像のプレビュー」)。
    thumbnail: bool,
}

impl ArchiveViewItem {
    /// 導出済みのアイテムから作る。プレビューの情報は、スキャンで読んで残した値を使い、ファイルは開かない
    /// (→ docs/archive.md「スキャン」)。まだ読んでいない行は、次のスキャンまでプレビューなしで並べる。
    /// その場で読むと、索引の大きいアーカイブでは一覧が開かず、再スキャンのボタンにも届かないため。
    fn new(item: &IndexedItem, subtitle: Option<String>) -> Self {
        let file_name = item.file_name().to_string();
        let preview = preview_of(&file_name, item.preview);
        Self {
            id: item.id,
            title: item.title.clone(),
            file_name,
            rel_path: item.rel_path.clone(),
            subtitle,
            image: preview.image,
            is_text: preview.is_text,
            thumbnail: preview.thumbnail,
        }
    }
}

fn preview_of(file_name: &str, stored: StoredPreview) -> thumbnails::FilePreview {
    if stored.read {
        thumbnails::stored_preview(file_name, stored.image, stored.is_text)
    } else {
        thumbnails::FilePreview {
            image: None,
            is_text: false,
            thumbnail: false,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ArchiveViewResponse {
    archive_title: String,
    /// ルートに近い順の祖先グループ。自分自身は含まない。用途は
    /// `FolderBrowseResponse::ancestors` と同じ。
    ancestors: Vec<super::contents::GroupAncestor>,
    axes: Vec<ArchiveAxisResponse>,
    /// 区切った続き (`/archive/page`) を引くときに渡す、絞って並べた結果の名前。
    snapshot: String,
    /// 絞り込み後の件数。
    total: usize,
    /// 軸で絞った後、名前の語で絞る前の数。
    total_all: usize,
    /// 先頭の区切り。引いた時点で消えた・非公開になった行は `null` (→ docs/archive.md「一覧の API を区切って返す」)。
    items: Vec<Option<ArchiveViewItem>>,
    /// アーカイブの登録先のフォルダーが見つからない。画面は空の一覧の代わりにそう出す。
    folder_missing: bool,
    /// 見ている人がこのコンテンツを書き換えられるか。画面は真のとき、編集ページへの導線を出す。
    can_edit: bool,
}

/// 一覧の最初の要求のクエリ (→ docs/archive.md「一覧の API を区切って返す」)。
/// 軸の絞り込みは `axis.<軸名>=<表示名>`、ほかは `sort`・`filter` (名前の語)・`status` (管理用)・`limit`。
fn parse_list_query(params: &HashMap<String, String>, match_paths: bool) -> (ListQuery, usize) {
    let filters = params
        .iter()
        .filter_map(|(key, value)| {
            Some((
                key.strip_prefix(AXIS_QUERY_PREFIX)?.to_string(),
                value.clone(),
            ))
        })
        .collect();
    let query = ListQuery {
        filters,
        terms: super::list_filter::parse_terms(params.get("filter").map_or("", String::as_str)),
        match_paths,
        status: if match_paths {
            StatusFilter::from_query(params.get("status").map(String::as_str))
        } else {
            StatusFilter::All
        },
        order: super::sort::ArchiveSortOrder::from_query(params.get("sort").map(String::as_str)),
    };
    (
        query,
        page_limit(params.get("limit").and_then(|limit| limit.parse().ok())),
    )
}

fn page_limit(limit: Option<usize>) -> usize {
    limit.map_or(PAGE_LIMIT, |limit| limit.clamp(1, PAGE_LIMIT))
}

fn snapshot_owner(viewer: &Viewer) -> Option<i64> {
    match viewer {
        Viewer::Anonymous => None,
        Viewer::User(user) => Some(user.id),
    }
}

/// 閲覧用のアーカイブ一覧の、最初の区切り。軸の定義・選択肢・件数と、絞って並べた結果の名前を返す
/// (→ docs/archive.md「エンドポイント一覧」・「一覧の API を区切って返す」)。
///
/// クエリの軸の絞り込みは `axis.<軸名>=<表示名>`。**効かないキー・値は無視する**
/// (422にしない。→ docs/archive.md「エンドポイント一覧」)。
/// utoipaはキーが動的なクエリを型付けできないため `params` には載せていない。
#[utoipa::path(
    get,
    path = "/contents/{id}/archive",
    params(("id" = i64, Path)),
    responses(
        (status = OK, body = ArchiveViewResponse, description = "軸の選択肢と、絞り込み後のアイテムの先頭の区切り"),
        (status = 401, body = crate::error::ErrorResponse, description = "閲覧にログインが必要"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しない・アーカイブでない"),
    )
)]
async fn view_archive(
    viewer: Viewer,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<ArchiveViewResponse>, AppError> {
    let (meta, lineage) = load_viewable_archive(&state.pool, &viewer, id).await?;
    // 下でフォルダーの中を直接読むので、つながらない NAS やディスクを先に見分ける。
    super::roots::ensure_reachable(&state, &meta.path).await?;
    // 区切りの続きでは走らせない。NAS の数千のフォルダーを区切りの数だけ見に行くことになるため。
    archive::refresh_changed_dirs(&state, id, &meta).await;
    let (query, limit) = parse_list_query(&params, false);

    let (versions, index) = archive_index::index(&state, id).await?;
    let (_, marks) = archive_index::marks(&state, id).await?;
    let root = PathBuf::from(&meta.path);
    let (folder_missing, result, index, marks) = run_blocking(move || {
        let folder_missing = folder_missing(&root);
        // 登録先ごと無ければ、どの行も開けないので並べない。行ごとの有無は確かめない (→ docs/archive.md「アイテムの配信」)。
        let result = archive_index::run_query(
            &index,
            &|item: &IndexedItem| !folder_missing && marks.is_public(&item.rel_path),
            true,
            &query,
        );
        (folder_missing, result, index, marks)
    })
    .await?;

    let snapshot = Snapshot {
        archive_id: id,
        kind: SnapshotKind::View,
        owner: snapshot_owner(&viewer),
        ids: result.ids.into(),
        positions: None,
        active_filters: result.active_filters.into(),
        versions,
    };
    let items = view_page(&index, &marks, &snapshot, 0, limit);
    let total = snapshot.ids.len();
    let token = state.archive_index.create_snapshot(snapshot)?;

    Ok(Json(ArchiveViewResponse {
        archive_title: meta.title,
        ancestors: lineage.breadcrumbs(),
        axes: axes_response(result.axes),
        snapshot: token,
        total,
        total_all: result.total_all,
        items,
        folder_missing,
        can_edit: lineage.editable_by(&viewer),
    }))
}

/// 閲覧用の区切りの行。**引くときの公開の状態でもう一度確かめる**。結果を作った後に非公開になった行の
/// 名前を出さないため。消えた行と非公開になった行は `None` (→ docs/archive.md「一覧の API を区切って返す」)。
fn view_page(
    index: &ArchiveIndex,
    marks: &Marks,
    snapshot: &Snapshot,
    offset: usize,
    limit: usize,
) -> Vec<Option<ArchiveViewItem>> {
    page_ids(snapshot, offset, limit)
        .iter()
        .map(|&id| {
            let item = index.get(id)?;
            marks
                .is_public(&item.rel_path)
                .then(|| ArchiveViewItem::new(item, index.subtitle(item, &snapshot.active_filters)))
        })
        .collect()
}

fn page_ids(snapshot: &Snapshot, offset: usize, limit: usize) -> &[i64] {
    let start = offset.min(snapshot.ids.len());
    let end = start.saturating_add(limit).min(snapshot.ids.len());
    &snapshot.ids[start..end]
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
struct PageQuery {
    /// 最初の要求が返した結果の名前。
    snapshot: String,
    offset: usize,
    limit: Option<usize>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ArchivePageResponse {
    /// 消えた・非公開になった行は `null`。
    items: Vec<Option<ArchiveViewItem>>,
    /// 結果を作った後に、索引か公開・非公開の印が変わったか。画面は一覧の上に取り直しを出す。
    stale: bool,
    /// 混ぜた結果 (`/archive/shuffle`) のときだけ、各行の混ぜる前の結果での位置。
    #[serde(skip_serializing_if = "Option::is_none")]
    positions: Option<Vec<usize>>,
}

/// 閲覧用の一覧の、区切りの続き (→ docs/archive.md「一覧の API を区切って返す」)。
#[utoipa::path(
    get,
    path = "/contents/{id}/archive/page",
    params(("id" = i64, Path), PageQuery),
    responses(
        (status = OK, body = ArchivePageResponse, description = "区切った行"),
        (status = 401, body = crate::error::ErrorResponse, description = "閲覧にログインが必要"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しない・アーカイブでない"),
        (status = 409, body = crate::error::ErrorResponse, description = "結果が捨てられた・別の人や口のもの。最初から取り直す"),
    )
)]
async fn view_archive_page(
    viewer: Viewer,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<PageQuery>,
) -> Result<Json<ArchivePageResponse>, AppError> {
    load_viewable_archive(&state.pool, &viewer, id).await?;
    let snapshot = state
        .archive_index
        .snapshot(
            &query.snapshot,
            id,
            SnapshotKind::View,
            snapshot_owner(&viewer),
        )
        .ok_or_else(snapshot_gone)?;
    let (versions, index) = archive_index::index(&state, id).await?;
    let (_, marks) = archive_index::marks(&state, id).await?;
    let limit = page_limit(query.limit);
    let items = view_page(&index, &marks, &snapshot, query.offset, limit);
    Ok(Json(ArchivePageResponse {
        items,
        stale: versions != snapshot.versions,
        positions: page_positions(&snapshot, query.offset, limit),
    }))
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
struct ShuffleQuery {
    /// 混ぜる元の結果の名前 (一覧の最初の要求が返したもの)。
    snapshot: String,
    limit: Option<usize>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ArchiveShuffleResponse {
    /// 混ぜた結果の名前。続きは `/archive/page` で引く。
    snapshot: String,
    total: usize,
    /// 消えた・非公開になった行は `null`。
    items: Vec<Option<ArchiveViewItem>>,
    /// 各行の、混ぜる前の結果での位置。
    positions: Vec<usize>,
}

/// 閲覧用の一覧の結果のうち、音声の行だけを混ぜた順に並べた結果を作り、その先頭の区切りを返す
/// (→ docs/archive.md「音声のシャッフル」)。
#[utoipa::path(
    get,
    path = "/contents/{id}/archive/shuffle",
    params(("id" = i64, Path), ShuffleQuery),
    responses(
        (status = OK, body = ArchiveShuffleResponse, description = "混ぜた結果の先頭の区切り"),
        (status = 401, body = crate::error::ErrorResponse, description = "閲覧にログインが必要"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しない・アーカイブでない"),
        (status = 409, body = crate::error::ErrorResponse, description = "元の結果が捨てられた・別の人や口のもの。最初から取り直す"),
    )
)]
async fn view_archive_shuffle(
    viewer: Viewer,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<ShuffleQuery>,
) -> Result<Json<ArchiveShuffleResponse>, AppError> {
    load_viewable_archive(&state.pool, &viewer, id).await?;
    let source = state
        .archive_index
        .snapshot(
            &query.snapshot,
            id,
            SnapshotKind::View,
            snapshot_owner(&viewer),
        )
        .ok_or_else(snapshot_gone)?;
    let (shuffled, index, marks) = shuffle_snapshot(&state, id, source).await?;
    let limit = page_limit(query.limit);
    let items = view_page(&index, &marks, &shuffled, 0, limit);
    let positions = page_positions(&shuffled, 0, limit).unwrap_or_default();
    let total = shuffled.ids.len();
    let snapshot = state.archive_index.create_snapshot(shuffled)?;
    Ok(Json(ArchiveShuffleResponse {
        snapshot,
        total,
        items,
        positions,
    }))
}

/// `source` の音声の行を混ぜた結果と、区切りを組み立てるのに使う今の索引と印。
async fn shuffle_snapshot(
    state: &AppState,
    id: i64,
    source: Arc<Snapshot>,
) -> Result<(Snapshot, Arc<ArchiveIndex>, Arc<Marks>), AppError> {
    let (_, index) = archive_index::index(state, id).await?;
    let (_, marks) = archive_index::marks(state, id).await?;
    let seed = super::random_u64()?;
    run_blocking(move || {
        let shuffled = source.shuffled_audio(&index, seed);
        (shuffled, index, marks)
    })
    .await
}

/// 混ぜた結果なら、区切りの各行の混ぜる前の位置。
fn page_positions(snapshot: &Snapshot, offset: usize, limit: usize) -> Option<Vec<usize>> {
    let positions = snapshot.positions.as_deref()?;
    let start = offset.min(positions.len());
    let end = start.saturating_add(limit).min(positions.len());
    Some(positions[start..end].to_vec())
}

fn snapshot_gone() -> AppError {
    AppError::Conflict("the list snapshot has expired".to_string())
}

// --- GET /contents/{id}/manage-archive (管理用) ---

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ManageArchiveViewItem {
    #[serde(flatten)]
    item: ArchiveViewItem,
    published: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ArchiveStatusCounts {
    all: usize,
    published: usize,
    unpublished: usize,
}

impl From<StatusCounts> for ArchiveStatusCounts {
    fn from(counts: StatusCounts) -> Self {
        Self {
            all: counts.all,
            published: counts.published,
            unpublished: counts.unpublished,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ManageArchiveViewResponse {
    axes: Vec<ArchiveAxisResponse>,
    snapshot: String,
    total: usize,
    /// 軸で絞った後、名前の語と公開の状態で絞る前の数。
    total_all: usize,
    /// 軸と名前の語で絞ったうえでの、公開の状態ごとの件数。絞り込みの欄に出す。
    status_counts: ArchiveStatusCounts,
    /// 消えた行は `null`。
    items: Vec<Option<ManageArchiveViewItem>>,
    folder_missing: bool,
}

/// 管理画面のアイテムのタブの一覧の、最初の区切り。閲覧用の一覧 (`view_archive`) と同じ絞り込み・並べ替え・導出で、
/// 未公開と実体の無いアイテムも含める (→ docs/archive.md「エンドポイント一覧」)。
/// 閲覧者に見える姿を、管理画面を離れずに確かめるため。名前の語はパスでも照らし、公開の状態 (`status`) でも絞る。
#[utoipa::path(
    get,
    path = "/contents/{id}/manage-archive",
    params(("id" = i64, Path)),
    responses(
        (status = OK, body = ManageArchiveViewResponse, description = "未公開を含む、軸の選択肢と絞り込み後のアイテムの先頭の区切り"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "管理できない・存在しない・アーカイブでない"),
    )
)]
async fn manage_view_archive(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<ManageArchiveViewResponse>, AppError> {
    let meta = archive::load_manageable_archive(&state.pool, &user, id).await?;
    super::roots::ensure_reachable(&state, &meta.path).await?;
    archive::refresh_changed_dirs(&state, id, &meta).await;
    let (query, limit) = parse_list_query(&params, true);

    let (versions, index) = archive_index::index(&state, id).await?;
    let (_, marks) = archive_index::marks(&state, id).await?;
    let root = PathBuf::from(&meta.path);
    let (folder_missing, result, index, marks) = run_blocking(move || {
        let folder_missing = folder_missing(&root);
        let result = archive_index::run_query(
            &index,
            &|item: &IndexedItem| marks.is_public(&item.rel_path),
            false,
            &query,
        );
        (folder_missing, result, index, marks)
    })
    .await?;

    let snapshot = Snapshot {
        archive_id: id,
        kind: SnapshotKind::Manage,
        owner: Some(user.id),
        ids: result.ids.into(),
        positions: None,
        active_filters: result.active_filters.into(),
        versions,
    };
    let items = manage_page(&index, &marks, &snapshot, 0, limit);
    let total = snapshot.ids.len();
    let token = state.archive_index.create_snapshot(snapshot)?;

    Ok(Json(ManageArchiveViewResponse {
        axes: axes_response(result.axes),
        snapshot: token,
        total,
        total_all: result.total_all,
        status_counts: result.status_counts.into(),
        items,
        folder_missing,
    }))
}

fn manage_page(
    index: &ArchiveIndex,
    marks: &Marks,
    snapshot: &Snapshot,
    offset: usize,
    limit: usize,
) -> Vec<Option<ManageArchiveViewItem>> {
    page_ids(snapshot, offset, limit)
        .iter()
        .map(|&id| {
            let item = index.get(id)?;
            Some(ManageArchiveViewItem {
                published: marks.is_public(&item.rel_path),
                item: ArchiveViewItem::new(item, index.subtitle(item, &snapshot.active_filters)),
            })
        })
        .collect()
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ManageArchivePageResponse {
    items: Vec<Option<ManageArchiveViewItem>>,
    stale: bool,
    /// 混ぜた結果 (`/manage-archive/shuffle`) のときだけ、各行の混ぜる前の結果での位置。
    #[serde(skip_serializing_if = "Option::is_none")]
    positions: Option<Vec<usize>>,
}

/// 管理画面のアイテムのタブの一覧の、区切りの続き。
#[utoipa::path(
    get,
    path = "/contents/{id}/manage-archive/page",
    params(("id" = i64, Path), PageQuery),
    responses(
        (status = OK, body = ManageArchivePageResponse, description = "区切った行"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "管理できない・存在しない・アーカイブでない"),
        (status = 409, body = crate::error::ErrorResponse, description = "結果が捨てられた・別の人や口のもの。最初から取り直す"),
    )
)]
async fn manage_view_archive_page(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<PageQuery>,
) -> Result<Json<ManageArchivePageResponse>, AppError> {
    archive::load_manageable_archive(&state.pool, &user, id).await?;
    let snapshot = state
        .archive_index
        .snapshot(&query.snapshot, id, SnapshotKind::Manage, Some(user.id))
        .ok_or_else(snapshot_gone)?;
    let (versions, index) = archive_index::index(&state, id).await?;
    let (_, marks) = archive_index::marks(&state, id).await?;
    let limit = page_limit(query.limit);
    let items = manage_page(&index, &marks, &snapshot, query.offset, limit);
    Ok(Json(ManageArchivePageResponse {
        items,
        stale: versions != snapshot.versions,
        positions: page_positions(&snapshot, query.offset, limit),
    }))
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ManageArchiveShuffleResponse {
    /// 混ぜた結果の名前。続きは `/manage-archive/page` で引く。
    snapshot: String,
    total: usize,
    /// 消えた行は `null`。
    items: Vec<Option<ManageArchiveViewItem>>,
    /// 各行の、混ぜる前の結果での位置。
    positions: Vec<usize>,
}

/// 管理画面のアイテムのタブの一覧の、音声の行を混ぜた結果 (→ `view_archive_shuffle`)。
#[utoipa::path(
    get,
    path = "/contents/{id}/manage-archive/shuffle",
    params(("id" = i64, Path), ShuffleQuery),
    responses(
        (status = OK, body = ManageArchiveShuffleResponse, description = "混ぜた結果の先頭の区切り"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "管理できない・存在しない・アーカイブでない"),
        (status = 409, body = crate::error::ErrorResponse, description = "元の結果が捨てられた・別の人や口のもの。最初から取り直す"),
    )
)]
async fn manage_view_archive_shuffle(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<ShuffleQuery>,
) -> Result<Json<ManageArchiveShuffleResponse>, AppError> {
    archive::load_manageable_archive(&state.pool, &user, id).await?;
    let source = state
        .archive_index
        .snapshot(&query.snapshot, id, SnapshotKind::Manage, Some(user.id))
        .ok_or_else(snapshot_gone)?;
    let (shuffled, index, marks) = shuffle_snapshot(&state, id, source).await?;
    let limit = page_limit(query.limit);
    let items = manage_page(&index, &marks, &shuffled, 0, limit);
    let positions = page_positions(&shuffled, 0, limit).unwrap_or_default();
    let total = shuffled.ids.len();
    let snapshot = state.archive_index.create_snapshot(shuffled)?;
    Ok(Json(ManageArchiveShuffleResponse {
        snapshot,
        total,
        items,
        positions,
    }))
}

/// 登録先のフォルダーごと無いか。読めないだけのものは含めない。ファイルシステムを見るので、非同期のワーカーの外で呼ぶ。
pub(super) fn folder_missing(path: &std::path::Path) -> bool {
    std::fs::metadata(path).is_err_and(|err| err.kind() == std::io::ErrorKind::NotFound)
}

// --- 検索 (→ docs/search.md) ---

/// 検索の対象にするアーカイブ。閲覧者が一覧で見られることは、呼び出し側が確かめる。
pub(super) struct SearchableArchive {
    pub(super) id: i64,
    pub(super) title: String,
    pub(super) path: String,
    /// アーカイブより上のグループ。ルートに近い順。
    pub(super) ancestors: Vec<super::contents::GroupAncestor>,
}

/// 検索で当たったアーカイブのファイル1件と、並べ替えに使う値。
struct ItemHit {
    score: super::search::Score,
    /// `search_items` の `sources` の添字。
    source: usize,
    archive_id: i64,
    rel_path: String,
    sort_key: Vec<ValueKey>,
    item: ArchiveViewItem,
}

/// 検索の結果のアーカイブのファイルの並び (→ docs/search.md「結果の並びと上限」)。
/// 当たりのよい順で、同じ程度ならタイトル順、同じタイトルはアーカイブごとに、その一覧の既定の並び (軸の順) にする。
fn item_hit_cmp(a: &ItemHit, b: &ItemHit) -> std::cmp::Ordering {
    a.score
        .cmp(&b.score)
        .then_with(|| title_cmp(&a.item.title, &b.item.title))
        .then(a.archive_id.cmp(&b.archive_id))
        .then_with(|| a.sort_key.cmp(&b.sort_key))
        .then_with(|| a.rel_path.cmp(&b.rel_path))
}

/// アーカイブの公開アイテムを、表示タイトルと軸の値で探す。表示タイトル順に並べ、
/// `search::RESULT_LIMIT` 件で打ち切って、打ち切ったかを添えて返す。
/// 導出の結果はメモリに持ったものを使い、持っていないフォルダーは作るが持たない (`index_if_cached_or_build`)。
/// 検索は多くのフォルダーを一度に見るので、持つと開いている一覧の分を追い出すため。
pub(super) async fn search_items(
    state: &AppState,
    archives: Vec<SearchableArchive>,
    mut terms: super::search::Terms,
) -> Result<(Vec<(super::search::Score, SearchFileHit)>, bool), AppError> {
    let mut sources: Vec<(SearchableArchive, Arc<ArchiveIndex>, Arc<Marks>)> =
        Vec::with_capacity(archives.len());
    // つながらない NAS やディスクのアーカイブは、検索全体を待たせずに飛ばす。
    let paths: Vec<&str> = archives
        .iter()
        .map(|archive| archive.path.as_str())
        .collect();
    let reachable = super::roots::reachable_all(state, &paths, OTHERS_WAIT).await?;
    for (archive, _) in archives
        .into_iter()
        .zip(reachable)
        .filter(|(_, reachable)| *reachable)
    {
        let index = archive_index::index_if_cached_or_build(state, archive.id).await?;
        let (_, marks) = archive_index::marks(state, archive.id).await?;
        sources.push((archive, index, marks));
    }

    run_blocking(move || {
        // フォルダーごと見つからないアーカイブは、閲覧の画面と同じく行を出さない (→ docs/archive.md「アイテムの配信」)。
        let sources: Vec<_> = sources
            .into_iter()
            .filter(|(archive, _, _)| !folder_missing(std::path::Path::new(&archive.path)))
            .collect();
        let mut hits: Vec<ItemHit> = Vec::new();
        for (source, (archive, index, marks)) in sources.iter().enumerate() {
            let places = super::search::container_places(&archive.title, &archive.ancestors);
            let no_filters: Vec<Option<String>> = vec![None; index.axes.len()];
            // 公開判定は一覧と配信と同じ印を見る (→ docs/archive.md「アイテムの公開」)。
            for item in index
                .items
                .iter()
                .filter(|item| marks.is_public(&item.rel_path))
            {
                let title = super::search::normalize(&item.title);
                let values: Vec<String> = (0..index.axes.len())
                    .filter_map(|axis| index.value(item, axis))
                    .map(|value| super::search::normalize(&value.display))
                    .collect();
                // 途中のフォルダー名は照らさない。閲覧の画面で階層を見せないため (→ docs/archive.md)。軸の値が代わりになる。
                let mut fields = vec![(FieldKind::Own, title.as_str())];
                fields.extend(values.iter().map(|text| (FieldKind::Detail, text.as_str())));
                fields.extend(places.iter().map(|text| (FieldKind::Place, text.as_str())));
                let Some(found) = terms.score(&fields) else {
                    continue;
                };
                hits.push(ItemHit {
                    score: found.score,
                    source,
                    archive_id: archive.id,
                    rel_path: item.rel_path.clone(),
                    sort_key: (0..index.axes.len())
                        .map(|axis| {
                            index
                                .value(item, axis)
                                .map_or(ValueKey::Unset, |value| value.key.clone())
                        })
                        .collect(),
                    item: ArchiveViewItem::new(item, index.subtitle(item, &no_filters)),
                });
            }
        }

        hits.sort_by(item_hit_cmp);
        let truncated = hits.len() > super::search::RESULT_LIMIT;
        hits.truncate(super::search::RESULT_LIMIT);

        let hits = hits
            .into_iter()
            .map(|hit| {
                let (archive, _, _) = &sources[hit.source];
                let item = hit.item;
                // フォルダーの中のファイルの当たりと同じ行にする。配信はフォルダーの口が同じ印で確かめる (→ docs/folders.md「公開と非公開」)。
                let hit_row = SearchFileHit {
                    content_id: archive.id,
                    folder_title: archive.title.clone(),
                    ancestors: archive.ancestors.clone(),
                    path: item.rel_path,
                    entry: FolderEntry::indexed_file(
                        item.file_name,
                        item.image,
                        item.is_text,
                        item.thumbnail,
                    ),
                    listed: Some(ListedTitle {
                        title: item.title,
                        subtitle: item.subtitle,
                    }),
                };
                (hit.score, hit_row)
            })
            .collect();
        (hits, truncated)
    })
    .await
}

// --- 管理用: パスの例・公開切り替え ---

/// AI に渡す依頼文に載せるパスの数。フォルダーの構造とファイル名の付け方が分かれば足りる
/// (画面の `axes-ai-prompt` の `PATH_LIMIT` と同じ数)。
const PATH_SAMPLE_LIMIT: usize = 150;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct PathSamplesResponse {
    /// アイテムの数 (未公開を含む)。
    item_count: usize,
    /// パスを並べたうえで、間を空けて選んだ例。先頭だけだと最初のフォルダーのファイルに偏るため。
    rel_paths: Vec<String>,
}

/// 全体から間を空けて `limit` 件を選ぶ (画面の `axes-ai-prompt` の `spread` と同じ選び方)。
fn spread<T: Clone>(items: &[T], limit: usize) -> Vec<T> {
    if items.len() <= limit {
        return items.to_vec();
    }
    (0..limit)
        .map(|i| items[i * items.len() / limit].clone())
        .collect()
}

/// AI に渡す依頼文に載せる、アイテムのパスの例 (→ docs/archive.md「軸の定義を外部の AI で作る」)。
/// 依頼文に載せるのは例だけなので、全件を画面へ送らずにここで選ぶ (→ docs/archive.md「ほかに全件を前提にしているもの」)。
#[utoipa::path(
    get,
    path = "/contents/{id}/path-samples",
    params(("id" = i64, Path)),
    responses(
        (status = OK, body = PathSamplesResponse, description = "アイテムの数とパスの例"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "管理できない・存在しない・アーカイブでない"),
    )
)]
async fn path_samples(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<PathSamplesResponse>, AppError> {
    archive::load_manageable_archive(&state.pool, &user, id).await?;
    let mut rel_paths = sqlx::query_scalar!(
        "SELECT rel_path FROM archive_items WHERE archive_id = ?",
        id
    )
    .fetch_all(&state.pool)
    .await?;
    let response = run_blocking(move || {
        rel_paths.sort();
        PathSamplesResponse {
            item_count: rel_paths.len(),
            rel_paths: spread(&rel_paths, PATH_SAMPLE_LIMIT),
        }
    })
    .await?;
    Ok(Json(response))
}

/// まとめて切り替える対象。`itemIds` か、`snapshot` (と除く番号) のどちらか一方。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct BulkSetPublishedRequest {
    /// 選んだ行の番号。
    item_ids: Option<Vec<i64>>,
    /// 「すべて選択」したときの、画面が見ていた結果の名前 (管理用の一覧が返したもの)。
    snapshot: Option<String>,
    /// `snapshot` から除く番号 (すべて選択した後に外した行)。
    #[serde(default)]
    except_ids: Vec<i64>,
    published: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct BulkSetPublishedResponse {
    /// 切り替えた件数。結果を作った後に索引から消えた番号は数えない。
    updated: usize,
}

/// 公開フラグの一括切り替え (→ docs/archive.md「アイテムの公開」・「まとめて公開を切り替える」・「エンドポイント一覧」)。
///
/// `itemIds` で送るときは、所属しない id が1件でもあれば、どの行も更新せず404にする
/// (単体切り替えと同じ「所属しない id は404」を一括の場合も全件成立が条件にする)。
///
/// `snapshot` で送るときは、**画面が見ていた結果の番号そのもの**を対象にし、絞り込みをやり直さない。
/// やり直すと、その間に表示名が変わって絞り込みが効かなくなったときや新しいファイルが入ったときに、
/// 見ていないものまで切り替えてしまうため。結果を作った後に索引から消えた番号は飛ばす
/// (消えたファイルに切り替える中身は無く、1件のために全体を断ると押し直しても同じになるため)。
#[utoipa::path(
    put,
    path = "/contents/{id}/items",
    params(("id" = i64, Path)),
    request_body = BulkSetPublishedRequest,
    responses(
        (status = OK, body = BulkSetPublishedResponse, description = "更新した"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "`user` から見て他人が作ったもの"),
        (status = 404, body = crate::error::ErrorResponse, description = "所属しないitem_idを含む"),
        (status = 409, body = crate::error::ErrorResponse, description = "結果が捨てられた・別の人のもの"),
        (status = 422, body = crate::error::ErrorResponse, description = "itemIds と snapshot のどちらか一方を送っていない"),
    )
)]
async fn bulk_set_published(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<BulkSetPublishedRequest>,
) -> Result<Json<BulkSetPublishedResponse>, AppError> {
    // 全選択の対象は、鍵を取る前に索引から引く。索引を作り直すと数秒かかり、その間ほかの書き込みを塞ぐため。
    // 鍵を取った後に索引が変わっても、画面が見ていた結果の番号を切り替えるのは同じ (下の `snapshot` の説明)。
    let snapshot_targets = match (&payload.item_ids, payload.snapshot) {
        (Some(_), None) => None,
        (None, Some(token)) => {
            // 書き換えられない人に索引を作らせないよう、先に一度確かめる (書く前に鍵の中でもう一度確かめる)。
            archive::ensure_editable_archive(&state.pool, &user, id).await?;
            let snapshot = state
                .archive_index
                .snapshot(&token, id, SnapshotKind::Manage, Some(user.id))
                .ok_or_else(snapshot_gone)?;
            let (_, index) = archive_index::index(&state, id).await?;
            let except_ids = payload.except_ids;
            Some(
                run_blocking(move || {
                    let except: std::collections::HashSet<i64> = except_ids.into_iter().collect();
                    snapshot
                        .ids
                        .iter()
                        .filter(|id| !except.contains(id))
                        .filter_map(|&id| Some((index.get(id)?.rel_path.clone(), false)))
                        .collect::<Vec<_>>()
                })
                .await?,
            )
        }
        _ => {
            return Err(AppError::Validation(
                "send either itemIds or snapshot".to_string(),
            ));
        }
    };

    // 公開範囲の判定から書き込みまでを`contents_write_lock`で直列化する。判定の後に
    // 他人の`private`へ変わっても書き込まないため (公開範囲の更新は同じ鍵を取る)。
    let _write_guard = state.contents_write_lock.lock().await;
    archive::ensure_editable_archive(&state.pool, &user, id).await?;

    let targets: Vec<(String, bool)> = if let Some(targets) = snapshot_targets {
        targets
    } else {
        let item_ids = payload.item_ids.unwrap_or_default();
        let mut targets = Vec::with_capacity(item_ids.len());
        for item_id in &item_ids {
            targets.push((item_rel_path(&state.pool, id, *item_id).await?, false));
        }
        targets
    };
    let updated = targets.len();
    super::folder_marks::set_marks(&state.pool, id, targets, payload.published).await?;

    Ok(Json(BulkSetPublishedResponse { updated }))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct SetPublishedRequest {
    published: bool,
}

/// 公開フラグの単体切り替え (→ docs/archive.md「アイテムの公開」・「エンドポイント一覧」)。
/// DB の行を書き換えるだけなので、フォルダーのある NAS やディスクにつながらなくても切り替えられる。
#[utoipa::path(
    put,
    path = "/contents/{id}/items/{item_id}",
    params(("id" = i64, Path), ("item_id" = i64, Path)),
    request_body = SetPublishedRequest,
    responses(
        (status = 204, description = "更新した"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "`user` から見て他人が作ったもの"),
        (status = 404, body = crate::error::ErrorResponse, description = "所属しない・存在しないアイテム"),
    )
)]
async fn set_published(
    user: AuthUser,
    State(state): State<AppState>,
    Path((id, item_id)): Path<(i64, i64)>,
    AppJson(payload): AppJson<SetPublishedRequest>,
) -> Result<StatusCode, AppError> {
    // 鍵を取る理由は`bulk_set_published`と同じ。
    let _write_guard = state.contents_write_lock.lock().await;
    archive::ensure_editable_archive(&state.pool, &user, id).await?;
    let rel_path = item_rel_path(&state.pool, id, item_id).await?;
    super::folder_marks::set_marks(&state.pool, id, vec![(rel_path, false)], payload.published)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// アイテムの、登録先からの相対パス。このフォルダーのアイテムでなければ404。
async fn item_rel_path(
    pool: &sqlx::SqlitePool,
    archive_id: i64,
    item_id: i64,
) -> Result<String, AppError> {
    sqlx::query_scalar!(
        "SELECT rel_path FROM archive_items WHERE id = ? AND archive_id = ?",
        item_id,
        archive_id
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

// --- 配信 ---

/// アイテムの配信 (→ docs/archive.md「アイテムの配信」)。相対パスをクエリで受けず、`item_id` から
/// `rel_path` を引く。実際のファイル読み出しは `folder` と同じ
/// `resolve_path` + `serve_file_response` を共有する。
#[utoipa::path(
    get,
    path = "/contents/{id}/items/{item_id}/download",
    params(("id" = i64, Path), ("item_id" = i64, Path)),
    responses(
        (status = OK, description = "ファイルの内容"),
        (status = 401, body = crate::error::ErrorResponse, description = "閲覧にログインが必要"),
        (status = 404, body = crate::error::ErrorResponse, description = "所属しない・存在しない・非公開のアイテム、または実体が無い"),
    )
)]
async fn download_item(
    viewer: Viewer,
    State(state): State<AppState>,
    Path((id, item_id)): Path<(i64, i64)>,
    request: Request,
) -> Result<Response, AppError> {
    let file = item_file(&state, &viewer, id, item_id).await?;
    Ok(file.download(request).await)
}

#[derive(Debug, Deserialize, IntoParams)]
struct ItemPathQuery {
    /// アーカイブの登録先からの相対パス。`/` 区切りで、アイテムの `relPath` と同じ形。
    path: String,
}

/// 相対パスで指したアイテムの配信 (→ docs/archive.md「アイテムの配信」)。Markdown のプレビューが、
/// 相対パスの画像とリンクを開くのに使う。見える条件は `download_item` と同じ。
#[utoipa::path(
    get,
    path = "/contents/{id}/archive/download",
    params(("id" = i64, Path), ItemPathQuery),
    responses(
        (status = OK, description = "ファイルの内容"),
        (status = 401, body = crate::error::ErrorResponse, description = "閲覧にログインが必要"),
        (status = 404, body = crate::error::ErrorResponse, description = "索引に無い・非公開のアイテム、または実体が無い"),
    )
)]
async fn download_item_by_path(
    viewer: Viewer,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<ItemPathQuery>,
    request: Request,
) -> Result<Response, AppError> {
    let item_id = sqlx::query_scalar!(
        r#"SELECT id as "id!" FROM archive_items WHERE archive_id = ? AND rel_path = ?"#,
        id,
        query.path
    )
    .fetch_optional(&state.pool)
    .await?;
    // 公開範囲の判定は、アイテムが無いときも先に通す。見えないアーカイブで、パスがあるかを答えないため。
    let Some(item_id) = item_id else {
        load_viewable_archive(&state.pool, &viewer, id).await?;
        return Err(AppError::NotFound);
    };
    let file = item_file(&state, &viewer, id, item_id).await?;
    Ok(file.download(request).await)
}

/// 管理画面からのアイテムの配信 (→ docs/archive.md「アイテムの配信」)。未公開も渡す。
/// 辞書を決めるときに、公開せずに中身を確かめるためのもの。
/// 閲覧用の `download_item` の判定 (未公開は隠す) とは混ぜず、管理の権限で別に判定する。
#[utoipa::path(
    get,
    path = "/contents/{id}/items/{item_id}/manage-download",
    params(("id" = i64, Path), ("item_id" = i64, Path)),
    responses(
        (status = OK, description = "ファイルの内容"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "管理できない・所属しない・存在しないアイテム、または実体が無い"),
    )
)]
async fn manage_download_item(
    user: AuthUser,
    State(state): State<AppState>,
    Path((id, item_id)): Path<(i64, i64)>,
    request: Request,
) -> Result<Response, AppError> {
    let file = manage_item_file(&state, &user, id, item_id).await?;
    Ok(file.download(request).await)
}

/// 管理画面からの、相対パスで指したアイテムの配信 (→ docs/archive.md「アイテムの配信」)。未公開も渡す。
/// 管理画面の Markdown のプレビューが、未公開の画像も出すのに使う。
#[utoipa::path(
    get,
    path = "/contents/{id}/archive/manage-download",
    params(("id" = i64, Path), ItemPathQuery),
    responses(
        (status = OK, description = "ファイルの内容"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "管理できない・索引に無いアイテム、または実体が無い"),
    )
)]
async fn manage_download_item_by_path(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<ItemPathQuery>,
    request: Request,
) -> Result<Response, AppError> {
    let meta = archive::load_manageable_archive(&state.pool, &user, id).await?;
    let exists = sqlx::query_scalar!(
        r#"SELECT 1 as "one!: i64" FROM archive_items WHERE archive_id = ? AND rel_path = ?"#,
        id,
        query.path
    )
    .fetch_optional(&state.pool)
    .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }
    let file = contents::resolve_file_under(&state, &meta.path, &query.path, None).await?;
    Ok(file.download(request).await)
}

/// 管理画面のアイテムの縮小画像 (→ docs/ui.md「画像のプレビュー」)。見える条件は `manage_download_item` と同じ。
#[utoipa::path(
    get,
    path = "/contents/{id}/items/{item_id}/manage-thumbnail",
    params(("id" = i64, Path), ("item_id" = i64, Path)),
    responses(
        (status = OK, description = "縮小画像 (JPEG か PNG)"),
        (status = 304, description = "ETag が一致した"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "管理できない・所属しない・存在しないアイテム、実体が無い、またはプレビューしない画像"),
    )
)]
async fn manage_thumbnail_item(
    user: AuthUser,
    State(state): State<AppState>,
    Path((id, item_id)): Path<(i64, i64)>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let file = manage_item_file(&state, &user, id, item_id).await?;
    file.thumbnail(&state, &headers).await
}

/// 管理の権限で、未公開も含めてアイテムの実体を引く。閲覧用の `item_file` とは判定を混ぜない
/// (→ docs/archive.md「アイテムの配信」)。
async fn manage_item_file(
    state: &AppState,
    user: &AuthUser,
    id: i64,
    item_id: i64,
) -> Result<contents::ServedFile, AppError> {
    let meta = archive::load_manageable_archive(&state.pool, user, id).await?;
    let rel_path = sqlx::query_scalar!(
        "SELECT rel_path FROM archive_items WHERE id = ? AND archive_id = ?",
        item_id,
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    contents::resolve_file_under(state, &meta.path, &rel_path, None).await
}

/// アイテムの縮小画像 (→ docs/ui.md「画像のプレビュー」)。見える条件は `download_item` と同じ。
#[utoipa::path(
    get,
    path = "/contents/{id}/items/{item_id}/thumbnail",
    params(("id" = i64, Path), ("item_id" = i64, Path)),
    responses(
        (status = OK, description = "縮小画像 (JPEG か PNG)"),
        (status = 304, description = "ETag が一致した"),
        (status = 401, body = crate::error::ErrorResponse, description = "閲覧にログインが必要"),
        (status = 404, body = crate::error::ErrorResponse, description = "所属しない・存在しない・非公開のアイテム、実体が無い、またはプレビューしない画像"),
    )
)]
async fn thumbnail_item(
    viewer: Viewer,
    State(state): State<AppState>,
    Path((id, item_id)): Path<(i64, i64)>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let file = item_file(&state, &viewer, id, item_id).await?;
    file.thumbnail(&state, &headers).await
}

pub(super) async fn item_file(
    state: &AppState,
    viewer: &Viewer,
    id: i64,
    item_id: i64,
) -> Result<contents::ServedFile, AppError> {
    let (meta, _) = load_viewable_archive(&state.pool, viewer, id).await?;

    let rel_path = item_rel_path(&state.pool, id, item_id).await?;

    // 非公開はログイン済みの閲覧者にも隠す (→ docs/archive.md「アイテムの公開」、docs/access.md「匿名閲覧の受け口」)。
    // 一覧と検索 (`view_page`・`search_items`) も同じ条件を見るので、片方を変えるときはもう片方も直す。
    if !archive_index::marks(state, id)
        .await?
        .1
        .is_public(&rel_path)
    {
        return Err(AppError::NotFound);
    }

    contents::resolve_file_under(state, &meta.path, &rel_path, None).await
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    // ブラウザーが直接開くルートだけ、エラーを画面へのリダイレクトに変える
    // (→ `api::browser`)。他のルートを巻き込まないよう別に組み立てて merge する。
    let download_routes = OpenApiRouter::new()
        .routes(routes!(download_item))
        .routes(routes!(download_item_by_path))
        .routes(routes!(manage_download_item_by_path))
        .routes(routes!(manage_download_item))
        .layer(axum::middleware::from_fn(
            super::browser::redirect_errors_for_browsers,
        ));

    OpenApiRouter::new()
        .routes(routes!(view_archive))
        .routes(routes!(view_archive_page))
        .routes(routes!(view_archive_shuffle))
        .routes(routes!(manage_view_archive))
        .routes(routes!(manage_view_archive_page))
        .routes(routes!(manage_view_archive_shuffle))
        .routes(routes!(path_samples))
        .routes(routes!(bulk_set_published))
        .routes(routes!(set_published))
        .routes(routes!(thumbnail_item))
        .routes(routes!(manage_thumbnail_item))
        .merge(download_routes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::archive::{AxisDict, AxisDictRow, AxisIndex};
    use crate::api::archive_index::ItemRow;

    fn item_hit(title: &str, archive_id: i64, year: &str, rel_path: &str) -> ItemHit {
        // 当たりの良さは揃え、タイトルから先の並びを確かめる。
        let score = super::super::search::Terms::parse("a")
            .expect("検索語がある")
            .score(&[(FieldKind::Own, "a")])
            .expect("当たる")
            .score;
        ItemHit {
            score,
            source: 0,
            archive_id,
            rel_path: rel_path.to_string(),
            sort_key: vec![ValueKey::Raw(year.to_string())],
            item: ArchiveViewItem {
                id: 0,
                title: title.to_string(),
                file_name: String::new(),
                rel_path: rel_path.to_string(),
                subtitle: None,
                image: None,
                is_text: false,
                thumbnail: false,
            },
        }
    }

    /// 間を空けて選ぶ。数が上限以下ならそのまま返す。
    #[test]
    fn spread_picks_evenly_spaced_items() {
        let items: Vec<i32> = (0..10).collect();
        assert_eq!(spread(&items, 4), [0, 2, 5, 7]);
        assert_eq!(spread(&items[..3], 4), [0, 1, 2]);
    }

    /// タイトル順で、同じタイトルはアーカイブごとに軸の順に並ぶ (id の順ではない)。
    #[test]
    fn item_hit_cmp_orders_by_title_then_archive_then_axes() {
        let mut hits = [
            item_hit("第2回", 1, "2024", "2024/b.mp3"),
            item_hit("第1回", 2, "2023", "2023/a.mp3"),
            item_hit("第1回", 1, "2025", "2025/a.mp3"),
            item_hit("第1回", 1, "2023", "2023/z.mp3"),
            item_hit("第10回", 1, "2023", "2023/c.mp3"),
        ];
        hits.sort_by(item_hit_cmp);
        let order: Vec<&str> = hits.iter().map(|hit| hit.rel_path.as_str()).collect();
        assert_eq!(
            order,
            [
                "2023/z.mp3",
                "2025/a.mp3",
                "2023/a.mp3",
                "2024/b.mp3",
                "2023/c.mp3"
            ]
        );
    }
    use crate::api::archive_axes::{AxisMatchPosition, AxisSource};
    use crate::api::sort::ArchiveSortOrder;

    /// 軸を1本作る。`values` は辞書の行で、並べた順を `position` にする。
    fn axis(
        name: &str,
        source: AxisSource,
        dir_level: Option<i64>,
        filterable: bool,
        values: &[(&str, Option<&str>)],
    ) -> AxisIndex {
        let rows = values
            .iter()
            .zip(0..)
            .map(|((raw, display), position)| AxisDictRow {
                raw_value: raw.to_string(),
                display_name: display.map(str::to_string),
                position,
                match_position: AxisMatchPosition::Anywhere,
            })
            .collect();
        let dict = AxisDict::new(source, dir_level, rows).expect("軸の定義が正しくない");
        AxisIndex::new(name.to_string(), filterable, false, dict)
    }

    fn word_axis(name: &str, values: &[(&str, Option<&str>)]) -> AxisIndex {
        axis(name, AxisSource::FilenameWord, None, true, values)
    }

    fn level_axis(name: &str, dir_level: i64, values: &[(&str, Option<&str>)]) -> AxisIndex {
        axis(name, AxisSource::DirLevel, Some(dir_level), true, values)
    }

    fn unfilterable(name: &str, dir_level: i64, values: &[(&str, Option<&str>)]) -> AxisIndex {
        axis(name, AxisSource::DirLevel, Some(dir_level), false, values)
    }

    /// `(rel_path, created_at)` からアイテムを作る。id は並べた順。
    fn items_at(items: &[(&str, &str)]) -> Vec<ItemRow> {
        items
            .iter()
            .zip(1..)
            .map(|((rel_path, created_at), id)| ItemRow {
                id,
                rel_path: rel_path.to_string(),
                created_at: created_at.to_string(),
                ..ItemRow::default()
            })
            .collect()
    }

    fn items(rel_paths: &[&str]) -> Vec<ItemRow> {
        let with_dates: Vec<(&str, &str)> = rel_paths
            .iter()
            .map(|rel_path| (*rel_path, "2026-09-01T00:00:00.000Z"))
            .collect();
        items_at(&with_dates)
    }

    struct View {
        axes: Vec<ArchiveAxisResponse>,
        items: Vec<(String, ArchiveViewItem)>,
    }

    impl View {
        fn options(&self, axis: &str) -> Vec<(&str, usize)> {
            self.axes
                .iter()
                .find(|response| response.name == axis)
                .unwrap_or_else(|| panic!("軸 {axis} が返らなかった"))
                .options
                .iter()
                .map(|option| (option.value.as_str(), option.count))
                .collect()
        }

        fn axis_names(&self) -> Vec<&str> {
            self.axes.iter().map(|axis| axis.name.as_str()).collect()
        }

        fn rel_paths(&self) -> Vec<&str> {
            self.items
                .iter()
                .map(|(rel_path, _)| rel_path.as_str())
                .collect()
        }

        fn titles(&self) -> Vec<&str> {
            self.items
                .iter()
                .map(|(_, item)| item.title.as_str())
                .collect()
        }

        fn subtitles(&self) -> Vec<(&str, Option<&str>)> {
            self.items
                .iter()
                .map(|(_, item)| (item.title.as_str(), item.subtitle.as_deref()))
                .collect()
        }
    }

    /// 全部公開しているとして、閲覧用の一覧と同じく絞って並べる。
    fn view(
        axes: &[AxisIndex],
        rows: Vec<ItemRow>,
        template: Option<&str>,
        filters: &[(&str, &str)],
        order: ArchiveSortOrder,
    ) -> View {
        let query = ListQuery {
            filters: filters
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
            terms: Vec::new(),
            match_paths: false,
            status: StatusFilter::All,
            order,
        };
        let index = ArchiveIndex::build(rows, axes, template);
        let result = archive_index::run_query(&index, &|_| true, true, &query);
        let items = result
            .ids
            .iter()
            .map(|&id| {
                let item = index.get(id).expect("結果の番号は索引にある");
                (
                    item.rel_path.clone(),
                    ArchiveViewItem::new(item, index.subtitle(item, &result.active_filters)),
                )
            })
            .collect();
        View {
            axes: axes_response(result.axes),
            items,
        }
    }

    const SUBJECTS: &[(&str, Option<&str>)] = &[("kokugo", Some("国語")), ("eigo", Some("英語"))];
    const SUBJECT_FILES: &[&str] = &["2024_unknown.pdf", "2024_eigo.pdf", "2024_kokugo.pdf"];

    /// 絞り込みはアイテムを狭めるが、選択肢は絞り込み前の全体のまま保つ (→ docs/archive.md「軸の値の辞書と導出」)。
    #[test]
    fn filtering_narrows_items_but_not_options() {
        let axes = [word_axis("科目", SUBJECTS)];

        let filtered = view(
            &axes,
            items(SUBJECT_FILES),
            None,
            &[("科目", "国語")],
            ArchiveSortOrder::Axis,
        );

        assert_eq!(filtered.rel_paths(), vec!["2024_kokugo.pdf"]);
        assert_eq!(filtered.options("科目"), vec![("国語", 1), ("英語", 1)]);
    }

    /// 軸名に一致しないキー・どの選択肢の表示名にも一致しない値 (生の値を含む) は無視する
    /// (→ docs/archive.md「エンドポイント一覧」)。
    #[test]
    fn filters_that_match_nothing_are_ignored() {
        let axes = [word_axis("科目", SUBJECTS)];

        for filter in [("存在しない軸", "x"), ("科目", "kokugo")] {
            let ignored = view(
                &axes,
                items(SUBJECT_FILES),
                None,
                &[filter],
                ArchiveSortOrder::Axis,
            );
            assert_eq!(ignored.items.len(), 3, "{filter:?}");
        }
    }

    /// 既定は軸ベース (辞書の順 → 未設定)、`title` はタイトル順、`new` は見つかった日時の降順。
    /// どれも `rel_path` で決着させる (→ docs/archive.md「エンドポイント一覧」)。
    #[test]
    fn items_are_sorted_by_the_requested_order() {
        let axes = [word_axis("科目", SUBJECTS)];
        let rows = || {
            items_at(&[
                ("2024_kokugo.pdf", "2026-09-01T00:00:00.000Z"),
                ("2024_eigo.pdf", "2026-09-20T00:00:00.000Z"),
                ("2024_unknown.pdf", "2026-09-10T00:00:00.000Z"),
            ])
        };

        for (order, expected) in [
            (
                ArchiveSortOrder::Axis,
                ["2024_kokugo.pdf", "2024_eigo.pdf", "2024_unknown.pdf"],
            ),
            (
                ArchiveSortOrder::Title,
                ["2024_eigo.pdf", "2024_kokugo.pdf", "2024_unknown.pdf"],
            ),
            (
                ArchiveSortOrder::New,
                ["2024_eigo.pdf", "2024_unknown.pdf", "2024_kokugo.pdf"],
            ),
        ] {
            let sorted = view(&axes, rows(), None, &[], order);
            assert_eq!(sorted.rel_paths(), expected, "{order:?}");
        }
    }

    /// 未設定のアイテムは最後に来て、タイトルはファイル名に落ちる (→ docs/archive.md「表示タイトル」)。
    #[test]
    fn unset_items_come_last_with_the_file_name_as_title() {
        let axes = [word_axis("科目", SUBJECTS)];

        let titled = view(
            &axes,
            items(SUBJECT_FILES),
            Some("{科目}"),
            &[],
            ArchiveSortOrder::Axis,
        );

        assert_eq!(titled.titles(), vec!["国語", "英語", "2024_unknown.pdf"]);
    }

    /// 階層の軸は、辞書に無い生の値もそのまま選択肢に出て、絞り込みにも使える
    /// (→ docs/archive.md「軸の値の辞書と導出」)。
    #[test]
    fn raw_dir_level_values_are_options_and_filters() {
        let axes = [level_axis("年度", 1, &[("2024", Some("2024年度"))])];
        let rows = || items(&["2024/a.mp3", "2023/b.mp3"]);

        let all = view(&axes, rows(), None, &[], ArchiveSortOrder::Axis);
        assert_eq!(all.options("年度"), vec![("2024年度", 1), ("2023", 1)]);

        let filtered = view(
            &axes,
            rows(),
            None,
            &[("年度", "2023")],
            ArchiveSortOrder::Axis,
        );
        assert_eq!(filtered.rel_paths(), vec!["2023/b.mp3"]);
    }

    /// 選択肢の件数は、ほかの軸の絞り込みを効かせたまま数える。0件でも選択肢は消さない。
    /// 自分の軸の件数は、自分の絞り込みを外して数える (→ docs/archive.md「エンドポイント一覧」)。
    #[test]
    fn option_counts_apply_the_other_axes_filters() {
        let axes = [level_axis("年度", 1, &[]), level_axis("回", 2, &[])];
        let rows = || items(&["2024/1/a.mp3", "2024/2/a.mp3", "2023/1/a.mp3"]);

        let all = view(&axes, rows(), None, &[], ArchiveSortOrder::Axis);
        assert_eq!(all.options("回"), vec![("1", 2), ("2", 1)]);

        let filtered = view(
            &axes,
            rows(),
            None,
            &[("年度", "2023")],
            ArchiveSortOrder::Axis,
        );
        assert_eq!(filtered.options("回"), vec![("1", 1), ("2", 0)]);
        assert_eq!(filtered.options("年度"), vec![("2023", 1), ("2024", 2)]);
    }

    /// 選択肢は表示名でまとめ、表示名で絞り込む。絞り込みに出さない軸は返さず、その軸名の
    /// キーも無視するが、並び順には使う (→ docs/archive.md「軸の定義」・「軸の値の辞書と導出」)。
    #[test]
    fn options_are_grouped_by_display_name_and_unfilterable_axes_are_hidden() {
        let axes = [
            unfilterable("年度", 1, &[]),
            word_axis(
                "教科",
                &[
                    ("kagaku", Some("理科")),
                    ("kokugo", Some("国語")),
                    ("butsuri", Some("理科")),
                ],
            ),
        ];
        let rows = || items(&["2024/butsuri.pdf", "2024/kokugo.pdf", "2023/kagaku.pdf"]);

        let all = view(&axes, rows(), None, &[], ArchiveSortOrder::Axis);
        assert_eq!(all.axis_names(), vec!["教科"]);
        assert_eq!(all.options("教科"), vec![("理科", 2), ("国語", 1)]);
        assert_eq!(
            all.rel_paths(),
            vec!["2023/kagaku.pdf", "2024/butsuri.pdf", "2024/kokugo.pdf"]
        );

        let by_display = view(
            &axes,
            rows(),
            None,
            &[("教科", "理科")],
            ArchiveSortOrder::Axis,
        );
        assert_eq!(by_display.items.len(), 2);

        let unfilterable_key = view(
            &axes,
            rows(),
            None,
            &[("年度", "2024")],
            ArchiveSortOrder::Axis,
        );
        assert_eq!(unfilterable_key.items.len(), 3);
    }

    /// 見えるアイテムに値が1つも無い軸は返さない。選べるのが「すべて」だけの欄になるため
    /// (→ docs/archive.md「軸の値の辞書と導出」)。
    #[test]
    fn axes_without_any_value_among_visible_items_are_not_returned() {
        let axes = [
            level_axis("年度", 1, &[]),
            word_axis("状態", &[("draft", Some("下書き"))]),
        ];
        let all = view(
            &axes,
            items(&["2024/a.pdf", "2023/b.pdf"]),
            None,
            &[],
            ArchiveSortOrder::Axis,
        );
        assert_eq!(all.axis_names(), vec!["年度"]);
    }

    /// 2段目は、表示タイトルで使っていない軸のうち絞り込み中でない軸の値を、軸の順に表示名で並べる。
    /// 未設定の軸は出さず、出す値が無ければ `None`。タイトルがファイル名に落ちた行はテンプレートの軸も出し、
    /// 無視される絞り込みは絞り込み中に数えない (→ docs/ui.md「アーカイブの一覧画面」)。
    #[test]
    fn subtitles_show_unused_and_unfiltered_axis_values() {
        let axes = [
            level_axis("年度", 1, &[]),
            unfilterable("試験", 2, &[("hon", Some("本試験"))]),
            word_axis("科目", &[("kokugo", Some("国語"))]),
        ];
        let rows = || {
            items(&[
                "2024/hon/kokugo.pdf",
                "2024/hon/eigo.pdf",
                "2024/tsui/kokugo.pdf",
                "flat.pdf",
            ])
        };
        let template = Some("{試験} {科目}");

        let unfiltered = vec![
            ("本試験 国語", Some("2024")),
            ("eigo.pdf", Some("2024 本試験")),
            ("tsui 国語", Some("2024")),
            ("flat.pdf", None),
        ];
        for filters in [&[][..], &[("年度", "1999")][..], &[("試験", "本試験")][..]] {
            let viewed = view(&axes, rows(), template, filters, ArchiveSortOrder::Axis);
            assert_eq!(viewed.subtitles(), unfiltered, "{filters:?}");
        }

        let filtered = view(
            &axes,
            rows(),
            template,
            &[("年度", "2024")],
            ArchiveSortOrder::Axis,
        );
        assert_eq!(
            filtered.subtitles(),
            vec![
                ("本試験 国語", None),
                ("eigo.pdf", Some("本試験")),
                ("tsui 国語", None),
            ]
        );
    }
}
