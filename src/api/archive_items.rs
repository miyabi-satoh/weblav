//! アーカイブのアイテム一覧・公開切り替え・配信 (→ docs/archive.md「アイテムの公開」・「アイテムの配信」・「エンドポイント一覧」)。
//!
//! 属性値・表示タイトルの導出そのものは `archive` モジュールに置き、ここでは
//! 導出結果を絞り込み・並び替え・レスポンスの形に組み立てる。

use std::collections::HashMap;
use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::{AuthUser, Viewer};
use crate::error::{AppError, AppJson, run_blocking};
use crate::state::AppState;

use super::archive::{self, ArchiveMeta, ValueKey};
use super::contents;
use super::sort::title_cmp;
use super::thumbnails;

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

struct ItemRow {
    id: i64,
    rel_path: String,
    published: bool,
    /// スキャンでアイテムが見つかった日時 (→ `sort=new` の並び順)。
    created_at: String,
}

/// 閲覧者に見せるアイテム (閲覧用の一覧と検索)。公開判定は「アーカイブのvisibility」と
/// 「アイテムのpublished」の両方で (→ docs/archive.md「アイテムの公開」)、ここは後者だけを見る。
/// 配信 (`item_file`) も同じ条件を見るので、片方を変えるときはもう片方も直す。
async fn published_items(
    pool: &sqlx::SqlitePool,
    archive_id: i64,
) -> Result<Vec<ItemRow>, AppError> {
    Ok(sqlx::query_as!(
        ItemRow,
        r#"SELECT id as "id!", rel_path, published as "published: bool",
                  created_at as "created_at!" FROM archive_items
           WHERE archive_id = ? AND published = 1"#,
        archive_id
    )
    .fetch_all(pool)
    .await?)
}

// --- GET /contents/{id}/archive (閲覧用) ---

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ArchiveAxisOption {
    /// 表示名。表示名が同じ値は1つの選択肢にまとめる (→ docs/archive.md「軸の値の辞書と導出」)。
    /// 絞り込みのクエリにもこの値をそのまま載せる (→ docs/archive.md「エンドポイント一覧」)。
    value: String,
    /// **この選択肢を選んだときに残る件数** (→ docs/archive.md「エンドポイント一覧」)。
    /// 他の軸の現在の絞り込みは効かせたまま、この軸だけをこの値に置き換えて数える。
    /// 0件になる組み合わせを選ぶ前に見分けられるようにするためのもので、
    /// 選択肢自体は 0 でも消さない (緩める側の選択肢を消さない方針は変えない)。
    count: usize,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ArchiveAxisResponse {
    /// 絞り込みに出す軸だけを返す (→ docs/archive.md「軸の定義」)。
    name: String,
    /// **絞り込み前の**、閲覧者に見えるアイテム全体から作る (→ docs/archive.md「軸の値の辞書と導出」)。
    /// 選択肢を絞り込み条件では狭めない (緩める側の選択肢を消さないため)。
    options: Vec<ArchiveAxisOption>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ArchiveViewItem {
    id: i64,
    title: String,
    /// `rel_path` の最後の要素。ページ内プレイヤーへの振り分け (→ docs/ui.md「音声のページ内プレイヤー」) に使う。
    file_name: String,
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
    /// プレビューの情報はファイルを読むので空のまま作り、見せる行にだけ `with_preview` で足す。
    fn new(id: i64, derived: archive::DerivedItem, subtitle: Option<String>) -> Self {
        Self {
            id,
            title: derived.title,
            file_name: derived.file_name,
            subtitle,
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
    items: Vec<ArchiveViewItem>,
    /// アーカイブの登録先のフォルダーが見つからない。画面は空の一覧の代わりにそう出す。
    folder_missing: bool,
}

/// 閲覧用のアーカイブ一覧。軸の定義・選択肢・導出済みタイトル付きのアイテム一覧を
/// 1回のリクエストで返す (→ docs/archive.md「エンドポイント一覧」)。
///
/// クエリキーは軸名、値は選択肢の表示名 (`?科目=国語`)。**効かないキー・値は無視する**
/// (422にしない。→ docs/archive.md「エンドポイント一覧」)。
/// utoipaはキーが動的なクエリを型付けできないため `params` には載せていない。
#[utoipa::path(
    get,
    path = "/contents/{id}/archive",
    params(("id" = i64, Path)),
    responses(
        (status = OK, body = ArchiveViewResponse, description = "軸の選択肢と絞り込み後のアイテム一覧"),
        (status = 401, body = crate::error::ErrorResponse, description = "閲覧にログインが必要"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しない・アーカイブでない"),
    )
)]
async fn view_archive(
    viewer: Viewer,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(mut filters): Query<HashMap<String, String>>,
) -> Result<Json<ArchiveViewResponse>, AppError> {
    let (meta, lineage) = load_viewable_archive(&state.pool, &viewer, id).await?;
    let axes = archive::load_axis_index(&state.pool, id).await?;

    // `sort` は並び順の予約キーであり、軸の絞り込みには使わない
    // (→ docs/archive.md「エンドポイント一覧」、docs/ui.md「ホーム・グループ・フォルダー・アーカイブの並び順」)。軸フィルタとして解釈される前に取り除く。
    let order = super::sort::ArchiveSortOrder::from_query(filters.remove("sort").as_deref());

    // アーカイブのvisibilityは上のload_viewable_archiveで確認済みなので、アイテムのpublishedだけを見る。
    let rows = published_items(&state.pool, id).await?;

    // 導出は全アイテム × 軸 × 照合語の数だけ回るので、非同期のワーカーを塞がない。
    let template = meta.title_template;
    let root = PathBuf::from(&meta.path);
    let (axes, items, folder_missing) = run_blocking(move || {
        let folder_missing = folder_missing(&root);
        // 選択肢と件数も実体のある行だけで数えるよう、組み立ての前に外す。
        let (rows, sizes) = present_rows(&root, rows);
        let (axes, items) = build_archive_view(&axes, rows, template.as_deref(), &filters, order);
        let items = items
            .into_iter()
            .map(|(rel_path, item)| {
                let size = sizes[&item.id];
                with_preview(&root, &rel_path, size, item)
            })
            .collect::<Vec<_>>();
        (axes, items, folder_missing)
    })
    .await?;

    let ancestors = lineage.breadcrumbs();

    Ok(Json(ArchiveViewResponse {
        archive_title: meta.title,
        ancestors,
        axes,
        items,
        folder_missing,
    }))
}

/// アイテムの実体の情報。消えている・読めないものは `None`。ファイルを読むので、非同期のワーカーの外で呼ぶ。
///
/// 中身は渡さないので、アイテムごとに配信と同じ `resolve_path` は通さない。
/// 索引はリンクでないファイルだけを拾う (→ docs/archive.md「スキャン」) が、索引の後に
/// リンクへ差し替えられたものは辿らない。
fn item_metadata(root: &std::path::Path, rel_path: &str) -> Option<std::fs::Metadata> {
    std::fs::symlink_metadata(root.join(rel_path))
        .ok()
        .filter(|metadata| metadata.is_file())
}

/// 登録先のフォルダーごと無いか。読めないだけのものは含めない。ファイルシステムを見るので、非同期のワーカーの外で呼ぶ。
pub(super) fn folder_missing(path: &std::path::Path) -> bool {
    std::fs::metadata(path).is_err_and(|err| err.kind() == std::io::ErrorKind::NotFound)
}

/// 閲覧者に見せる行のうち、実体のあるものだけを残し、その大きさを `id` ごとに添える
/// (→ docs/archive.md「アイテムの配信」)。開けない行を並べないため。
fn present_rows(root: &std::path::Path, rows: Vec<ItemRow>) -> (Vec<ItemRow>, HashMap<i64, u64>) {
    let mut sizes = HashMap::new();
    let rows = rows
        .into_iter()
        .filter(|row| {
            let Some(metadata) = item_metadata(root, &row.rel_path) else {
                return false;
            };
            sizes.insert(row.id, metadata.len());
            true
        })
        .collect();
    (rows, sizes)
}

/// 行に、ページ内で見せるための情報 (画像の大きさ・テキストか) を埋める。ファイルを読むので、非同期のワーカーの外で呼ぶ。
fn with_preview(
    root: &std::path::Path,
    rel_path: &str,
    size: u64,
    item: ArchiveViewItem,
) -> ArchiveViewItem {
    let path = root.join(rel_path);
    let preview = thumbnails::file_preview(&item.file_name, &path, size);
    ArchiveViewItem {
        image: preview.image,
        is_text: preview.is_text,
        thumbnail: preview.thumbnail,
        ..item
    }
}

/// 行の2段目に出す軸の値 (→ docs/ui.md「アーカイブの一覧画面」)。表示タイトルで使っていない軸のうち、
/// 絞り込み中でない軸の値を、軸の並び順に空白で区切る。出す値が無ければ `None`。
fn item_subtitle(
    axes: &[archive::AxisIndex],
    derived: &archive::DerivedItem,
    template_axes: &std::collections::HashSet<&str>,
    active_filters: &[Option<&str>],
) -> Option<String> {
    let parts: Vec<&str> = axes
        .iter()
        .zip(&derived.axis_values)
        .zip(active_filters)
        .filter(|((axis, _), filter)| {
            filter.is_none()
                && !(derived.title_from_template && template_axes.contains(axis.name.as_str()))
        })
        .filter_map(|((_, value), _)| Some(value.as_ref()?.display.as_str()))
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// 閲覧用の一覧の、導出・絞り込み・並べ替え。`view_archive` から `run_blocking` で呼ぶ。
fn build_archive_view(
    axes: &[archive::AxisIndex],
    rows: Vec<ItemRow>,
    template: Option<&str>,
    filters: &HashMap<String, String>,
    order: super::sort::ArchiveSortOrder,
) -> (Vec<ArchiveAxisResponse>, Vec<(String, ArchiveViewItem)>) {
    let items: Vec<(i64, String, archive::DerivedItem, String)> = rows
        .into_iter()
        .map(|row| {
            let derived = archive::derive_item(&row.rel_path, axes, template);
            (row.id, row.rel_path, derived, row.created_at)
        })
        .collect();

    // 軸ごとの選択肢 (表示名 → 比較キー)。絞り込みに出さない軸は `None`。
    // 選択肢は絞り込み前の全体 (閲覧者に見えるアイテム全体) から作る
    // (→ docs/archive.md「軸の値の辞書と導出」)。現在の絞り込み条件では狭めない。
    let options_by_axis: Vec<Option<HashMap<String, ValueKey>>> = axes
        .iter()
        .enumerate()
        .map(|(i, axis)| {
            axis.filterable.then(|| {
                let mut options = HashMap::new();
                for (_, _, derived, _) in &items {
                    if let Some(value) = &derived.axis_values[i] {
                        options
                            .entry(value.display.clone())
                            .or_insert_with(|| value.key.clone());
                    }
                }
                options
            })
        })
        .collect();

    // 軸ごとに効かせる絞り込み。軸名に一致しないキー・絞り込みに出さない軸のキー・
    // どの選択肢にも一致しない値は無視する (→ docs/archive.md「エンドポイント一覧」)。
    let active_filters: Vec<Option<&str>> = axes
        .iter()
        .zip(&options_by_axis)
        .map(|(axis, options)| {
            let expected = filters.get(&axis.name)?;
            options
                .as_ref()?
                .contains_key(expected)
                .then_some(expected.as_str())
        })
        .collect();

    /// `derived` が、`skip_axis` 以外の全ての軸の絞り込み条件を満たすか。
    fn matches_filters(
        derived: &archive::DerivedItem,
        active_filters: &[Option<&str>],
        skip_axis: Option<usize>,
    ) -> bool {
        active_filters.iter().enumerate().all(|(i, expected)| {
            skip_axis == Some(i)
                || expected.is_none_or(|expected| {
                    derived.axis_values[i]
                        .as_ref()
                        .is_some_and(|value| value.display == expected)
                })
        })
    }

    let axes_response: Vec<ArchiveAxisResponse> = axes
        .iter()
        .zip(options_by_axis)
        .enumerate()
        .filter_map(|(i, (axis, options))| {
            // 件数は他の軸の絞り込みを効かせたまま数える。選択肢そのものは
            // 絞り込み前の全体から作る (両者で母集団が違う)。
            let mut counts: HashMap<&str, usize> = HashMap::new();
            for (_, _, derived, _) in &items {
                if let Some(value) = &derived.axis_values[i]
                    && matches_filters(derived, &active_filters, Some(i))
                {
                    *counts.entry(value.display.as_str()).or_default() += 1;
                }
            }
            // 表示名が違えば比較キーも違うので、キーだけで順が決まる。
            let mut options: Vec<(String, ValueKey)> = options?.into_iter().collect();
            options.sort_by(|a, b| a.1.cmp(&b.1));
            Some(ArchiveAxisResponse {
                name: axis.name.clone(),
                options: options
                    .into_iter()
                    .map(|(value, _)| ArchiveAxisOption {
                        count: counts.get(value.as_str()).copied().unwrap_or(0),
                        value,
                    })
                    .collect(),
            })
        })
        .collect();

    let mut matched: Vec<(i64, String, archive::DerivedItem, String)> = items
        .into_iter()
        .filter(|(_, _, derived, _)| matches_filters(derived, &active_filters, None))
        .collect();
    // 並び順は3択 (→ docs/archive.md「エンドポイント一覧」, docs/ui.md「ホーム・グループ・フォルダー・アーカイブの並び順」)。`Axis`(既定)は今までと同じ軸ベースの
    // 並びで、`rel_path`は最後のタイブレーク。`Title`/`New`も同じくタイブレークする。
    matched.sort_by(
        |(_, a_path, a_derived, a_created), (_, b_path, b_derived, b_created)| {
            match order {
                super::sort::ArchiveSortOrder::Axis => a_derived
                    .sort_key()
                    .cmp(&b_derived.sort_key())
                    .then_with(|| a_path.cmp(b_path)),
                super::sort::ArchiveSortOrder::Title => {
                    title_cmp(&a_derived.title, &b_derived.title).then_with(|| a_path.cmp(b_path))
                }
                // `created_at`はゼロ埋めした固定長のUTC文字列なので、辞書順が日時の順になる
                // (`sort::sort_rows`の`New`と同じ考え方)。
                super::sort::ArchiveSortOrder::New => {
                    b_created.cmp(a_created).then_with(|| a_path.cmp(b_path))
                }
            }
        },
    );

    // 2段目は、表示タイトルで使っていない軸のうち、絞り込み中でない軸の値 (→ docs/ui.md「アーカイブの一覧画面」)。
    // 絞り込みの状態で変わるので、ここで組み立てる。
    let template_axes = archive::template_placeholder_names(template);
    let items: Vec<(String, ArchiveViewItem)> = matched
        .into_iter()
        .map(|(id, rel_path, derived, _)| {
            let subtitle = item_subtitle(axes, &derived, &template_axes, &active_filters);
            (rel_path, ArchiveViewItem::new(id, derived, subtitle))
        })
        .collect();

    (axes_response, items)
}

// --- 検索 (→ docs/search.md) ---

/// 検索の対象にするアーカイブ。閲覧者が一覧で見られることは、呼び出し側が確かめる。
pub(super) struct SearchableArchive {
    pub(super) id: i64,
    pub(super) title: String,
    pub(super) path: String,
    pub(super) title_template: Option<String>,
}

/// 検索で当たったアーカイブのファイル1件と、並べ替えに使う値。
struct ItemHit {
    /// `search_items` の `sources` の添字。
    source: usize,
    archive_id: i64,
    rel_path: String,
    sort_key: Vec<ValueKey>,
    item: ArchiveViewItem,
}

/// 検索の結果のアーカイブのファイルの並び (→ docs/search.md「結果の並びと上限」)。
/// タイトル順で、同じタイトルはアーカイブごとに、その一覧の既定の並び (軸の順) にする。
fn item_hit_cmp(a: &ItemHit, b: &ItemHit) -> std::cmp::Ordering {
    title_cmp(&a.item.title, &b.item.title)
        .then(a.archive_id.cmp(&b.archive_id))
        .then_with(|| a.sort_key.cmp(&b.sort_key))
        .then_with(|| a.rel_path.cmp(&b.rel_path))
}

/// アーカイブの公開アイテムを、表示タイトルと軸の値で探す。表示タイトル順に並べ、
/// `search::RESULT_LIMIT` 件で打ち切って、打ち切ったかを添えて返す。
pub(super) async fn search_items(
    pool: &sqlx::SqlitePool,
    archives: Vec<SearchableArchive>,
    terms: super::search::Terms,
) -> Result<(Vec<super::search::SearchItemHit>, bool), AppError> {
    let mut sources = Vec::with_capacity(archives.len());
    for archive in archives {
        let axes = archive::load_axis_index(pool, archive.id).await?;
        let rows = published_items(pool, archive.id).await?;
        sources.push((archive, axes, rows));
    }

    // 導出は全アイテム × 軸 × 照合語の数だけ回り、行の情報はファイルを読むので、非同期のワーカーを塞がない。
    run_blocking(move || {
        let sources: Vec<_> = sources
            .into_iter()
            .map(|(archive, axes, rows)| {
                let (rows, sizes) = present_rows(std::path::Path::new(&archive.path), rows);
                (archive, axes, rows, sizes)
            })
            .collect();
        let mut hits: Vec<ItemHit> = Vec::new();
        for (index, (archive, axes, rows, _)) in sources.iter().enumerate() {
            let template = archive.title_template.as_deref();
            let template_axes = archive::template_placeholder_names(template);
            let no_filters = vec![None; axes.len()];
            for row in rows {
                let derived = archive::derive_item(&row.rel_path, axes, template);
                let mut fields = vec![super::search::normalize(&derived.title)];
                fields.extend(
                    derived
                        .axis_values
                        .iter()
                        .flatten()
                        .map(|value| super::search::normalize(&value.display)),
                );
                let fields: Vec<&str> = fields.iter().map(String::as_str).collect();
                if !terms.matches(&fields) {
                    continue;
                }
                let subtitle = item_subtitle(axes, &derived, &template_axes, &no_filters);
                hits.push(ItemHit {
                    source: index,
                    archive_id: archive.id,
                    rel_path: row.rel_path.clone(),
                    sort_key: derived.sort_key(),
                    item: ArchiveViewItem::new(row.id, derived, subtitle),
                });
            }
        }

        hits.sort_by(item_hit_cmp);
        let truncated = hits.len() > super::search::RESULT_LIMIT;
        hits.truncate(super::search::RESULT_LIMIT);

        let hits = hits
            .into_iter()
            .map(|hit| {
                let (archive, _, _, sizes) = &sources[hit.source];
                let size = sizes[&hit.item.id];
                super::search::SearchItemHit {
                    archive_id: archive.id,
                    archive_title: archive.title.clone(),
                    item: with_preview(
                        std::path::Path::new(&archive.path),
                        &hit.rel_path,
                        size,
                        hit.item,
                    ),
                }
            })
            .collect();
        (hits, truncated)
    })
    .await
}

// --- 管理用: 一覧・公開切り替え ---

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct AdminArchiveItemResponse {
    id: i64,
    rel_path: String,
    published: bool,
    title: String,
    /// ファイルが消えている・読めない。再スキャンすると一覧から外れる。
    missing: bool,
    /// ファイルが消えている・読めないときは `None`。
    size: Option<u64>,
    /// UNIXエポックミリ秒。取得できない場合は `None`。
    modified_at: Option<i64>,
}

/// `root` はアーカイブのフォルダー。大きさと更新日時をファイルから読むので、非同期のワーカーの外で呼ぶ。
fn to_admin_response(
    row: ItemRow,
    axes: &[archive::AxisIndex],
    template: Option<&str>,
    root: &std::path::Path,
) -> (Vec<ValueKey>, AdminArchiveItemResponse) {
    let derived = archive::derive_item(&row.rel_path, axes, template);
    let sort_key = derived.sort_key();
    // DB には持たず、その時点の値を出す (再スキャンまで古い値が出ないように)。
    // 閲覧と同じく、索引の後にリンクへ差し替えられたものは辿らない。
    let metadata = item_metadata(root, &row.rel_path);
    (
        sort_key,
        AdminArchiveItemResponse {
            id: row.id,
            rel_path: row.rel_path,
            published: row.published,
            title: derived.title,
            missing: metadata.is_none(),
            size: metadata.as_ref().map(|metadata| metadata.len()),
            modified_at: metadata.as_ref().and_then(contents::modified_at_millis),
        },
    )
}

/// 管理用のアイテム一覧。非公開も含めた全件を返す (→ docs/archive.md「エンドポイント一覧」)。
#[utoipa::path(
    get,
    path = "/contents/{id}/items",
    params(("id" = i64, Path)),
    responses(
        (status = OK, body = Vec<AdminArchiveItemResponse>, description = "非公開を含む全アイテム"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しない・アーカイブでない"),
    )
)]
async fn list_items(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<AdminArchiveItemResponse>>, AppError> {
    let meta = archive::load_manageable_archive(&state.pool, &user, id).await?;
    let axes = archive::load_axis_index(&state.pool, id).await?;

    let rows = sqlx::query_as!(
        ItemRow,
        r#"SELECT id as "id!", rel_path, published as "published: bool",
                  created_at as "created_at!" FROM archive_items WHERE archive_id = ?"#,
        id
    )
    .fetch_all(&state.pool)
    .await?;

    // 導出は全アイテム × 軸 × 照合語の数だけ回るので、非同期のワーカーを塞がない。
    let template = meta.title_template;
    let root = PathBuf::from(&meta.path);
    let items = run_blocking(move || {
        let mut items: Vec<(Vec<ValueKey>, AdminArchiveItemResponse)> = rows
            .into_iter()
            .map(|row| to_admin_response(row, &axes, template.as_deref(), &root))
            .collect();
        items.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.rel_path.cmp(&b.1.rel_path)));
        items.into_iter().map(|(_, item)| item).collect::<Vec<_>>()
    })
    .await?;

    Ok(Json(items))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct BulkSetPublishedRequest {
    item_ids: Vec<i64>,
    published: bool,
}

/// 公開フラグの一括切り替え (→ docs/archive.md「アイテムの公開」・「エンドポイント一覧」)。
///
/// `itemIds` に所属しない id が1件でもあれば、どの行も更新せず404にする
/// (単体切り替えと同じ「所属しない id は404」を一括の場合も全件成立が条件、
/// → docs/archive.md「エンドポイント一覧」)。`itemIds` が空なら何もせず204にする (「全件成立」が
/// 自明に真になるため)。1件ずつ`UPDATE`する構成のラウンドトリップ数は、
/// 1回のスキャンで索引できる件数の上限 (`ITEM_LIMIT`) で有界。
#[utoipa::path(
    put,
    path = "/contents/{id}/items",
    params(("id" = i64, Path)),
    request_body = BulkSetPublishedRequest,
    responses(
        (status = 204, description = "更新した"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "`user` から見て他人が作ったもの"),
        (status = 404, body = crate::error::ErrorResponse, description = "所属しないitem_idを含む"),
    )
)]
async fn bulk_set_published(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<BulkSetPublishedRequest>,
) -> Result<StatusCode, AppError> {
    // 公開範囲の判定から書き込みまでを`contents_write_lock`で直列化する。判定の後に
    // 他人の`private`へ変わっても書き込まないため (公開範囲の更新は同じ鍵を取る)。
    let _write_guard = state.contents_write_lock.lock().await;
    archive::ensure_editable_archive(&state.pool, &user, id).await?;

    let mut tx = crate::db::begin_write(&state.pool).await?;
    for item_id in &payload.item_ids {
        let result = sqlx::query!(
            "UPDATE archive_items SET published = ? WHERE id = ? AND archive_id = ?",
            payload.published,
            item_id,
            id
        )
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() != 1 {
            return Err(AppError::NotFound);
        }
    }
    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct SetPublishedRequest {
    published: bool,
}

/// 公開フラグの単体切り替え (→ docs/archive.md「アイテムの公開」・「エンドポイント一覧」)。
#[utoipa::path(
    put,
    path = "/contents/{id}/items/{item_id}",
    params(("id" = i64, Path), ("item_id" = i64, Path)),
    request_body = SetPublishedRequest,
    responses(
        (status = OK, body = AdminArchiveItemResponse, description = "更新後のアイテム"),
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
) -> Result<Json<AdminArchiveItemResponse>, AppError> {
    // 鍵を取る理由は`bulk_set_published`と同じ。
    let _write_guard = state.contents_write_lock.lock().await;
    let meta = archive::load_editable_archive(&state.pool, &user, id).await?;
    let axes = archive::load_axis_index(&state.pool, id).await?;

    let row = sqlx::query_as!(
        ItemRow,
        r#"UPDATE archive_items SET published = ? WHERE id = ? AND archive_id = ?
           RETURNING id as "id!", rel_path, published as "published: bool",
                     created_at as "created_at!""#,
        payload.published,
        item_id,
        id,
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let root = PathBuf::from(&meta.path);
    let (_, response) =
        run_blocking(move || to_admin_response(row, &axes, meta.title_template.as_deref(), &root))
            .await?;
    Ok(Json(response))
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
    let meta = archive::load_manageable_archive(&state.pool, &user, id).await?;
    let rel_path = sqlx::query_scalar!(
        "SELECT rel_path FROM archive_items WHERE id = ? AND archive_id = ?",
        item_id,
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let file = contents::resolve_file_under(&state, &meta.path, &rel_path).await?;
    Ok(file.download(request).await)
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

    let item = sqlx::query!(
        r#"SELECT rel_path, published as "published: bool"
           FROM archive_items WHERE id = ? AND archive_id = ?"#,
        item_id,
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    // 非公開はログイン済みの閲覧者にも隠す (→ docs/archive.md「アイテムの公開」、docs/access.md「匿名閲覧の受け口」)。
    // 一覧と検索 (published_items) も同じ条件を見るので、片方を変えるときはもう片方も直す。
    if !item.published {
        return Err(AppError::NotFound);
    }

    contents::resolve_file_under(state, &meta.path, &item.rel_path).await
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    // ブラウザーが直接開くルートだけ、エラーを画面へのリダイレクトに変える
    // (→ `api::browser`)。他のルートを巻き込まないよう別に組み立てて merge する。
    let download_routes = OpenApiRouter::new()
        .routes(routes!(download_item))
        .routes(routes!(manage_download_item))
        .layer(axum::middleware::from_fn(
            super::browser::redirect_errors_for_browsers,
        ));

    OpenApiRouter::new()
        .routes(routes!(view_archive))
        .routes(routes!(list_items, bulk_set_published))
        .routes(routes!(set_published))
        .routes(routes!(thumbnail_item))
        .merge(download_routes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::archive::{AxisDict, AxisDictRow, AxisIndex};

    fn item_hit(title: &str, archive_id: i64, year: &str, rel_path: &str) -> ItemHit {
        ItemHit {
            source: 0,
            archive_id,
            rel_path: rel_path.to_string(),
            sort_key: vec![ValueKey::Raw(year.to_string())],
            item: ArchiveViewItem {
                id: 0,
                title: title.to_string(),
                file_name: String::new(),
                subtitle: None,
                image: None,
                is_text: false,
                thumbnail: false,
            },
        }
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

    /// `(rel_path, created_at)` から公開済みのアイテムを作る。id は並べた順。
    fn items_at(items: &[(&str, &str)]) -> Vec<ItemRow> {
        items
            .iter()
            .zip(1..)
            .map(|((rel_path, created_at), id)| ItemRow {
                id,
                rel_path: rel_path.to_string(),
                published: true,
                created_at: created_at.to_string(),
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

    fn view(
        axes: &[AxisIndex],
        rows: Vec<ItemRow>,
        template: Option<&str>,
        filters: &[(&str, &str)],
        order: ArchiveSortOrder,
    ) -> View {
        let filters = filters
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        let (axes, items) = build_archive_view(axes, rows, template, &filters, order);
        View { axes, items }
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
