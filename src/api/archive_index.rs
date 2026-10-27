//! 一覧の表示の、導出済みの全アイテムと公開・非公開の印をメモリに持つ所と、区切って返すための
//! 結果の集まり (→ docs/archive.md「大きなアーカイブの一覧」)。
//!
//! 一覧を区切って返すと、1つの一覧を何度にも分けて取る。区切るたびに全件を導出すると数十万件で数秒かかるので、
//! 導出の結果を版 (`archive_versions`、トリガーで進む) と一緒に持ち、版が変わったときだけ作り直す。

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use crate::error::{AppError, run_blocking};
use crate::state::AppState;

use super::archive::{self, AxisIndex, DerivedAxisValue, ValueKey};
use super::folder_marks::Marks;
use super::sort::{ArchiveSortOrder, title_cmp};
use super::thumbnails;

/// 使われなくなった導出結果・結果の集まりを捨てるまでの時間。授業で開く一覧は続けて使われ、
/// 捨てても作り直すのは数秒なので、メモリを長く塞がない方を採る。
const IDLE_TTL: Duration = Duration::from_secs(10 * 60);

/// 同時に持つフォルダーの数。授業で開く一覧は数個に限られ、30万件で数十MBになるため。
const MAX_ENTRIES: usize = 4;

/// 結果の集まりを持つ量の上限 (番号の列のバイト数)。閲覧は匿名でもでき、絞り込みを変えるたびに作られるので、
/// 数でなく量で縛る。64MB は30万件の結果で約25個ぶん。
const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;

/// 結果の集まり1つに、番号の列とは別に数える量。名前・軸の絞り込みの値・表の行のぶん。
/// 0件の結果も量に数え、上限で数を縛るため。4KB で、上限まで溜まっても約1.6万個。
const SNAPSHOT_OVERHEAD_BYTES: usize = 4 * 1024;

/// 索引の1行。`published` はフォルダーの公開・非公開の印から決める (→ docs/folders.md「公開と非公開」)。
#[derive(Default)]
pub(super) struct ItemRow {
    pub(super) id: i64,
    pub(super) rel_path: String,
    /// スキャンでアイテムが見つかった日時 (→ `sort=new` の並び順)。
    pub(super) created_at: String,
    /// 以下はスキャンで読んで残した値 (→ docs/archive.md「スキャン」)。`file_size` が `None` ならまだ読んでいない。
    pub(super) file_size: Option<i64>,
    pub(super) modified_ns: Option<i64>,
    pub(super) image_width: Option<i64>,
    pub(super) image_height: Option<i64>,
    pub(super) is_text: bool,
    /// 写真の撮影日 (Unix 秒)。日付の軸が使う。
    pub(super) taken_at: Option<i64>,
}

impl ItemRow {
    pub(super) fn facts(&self) -> archive::ItemFacts<'_> {
        archive::ItemFacts {
            rel_path: &self.rel_path,
            dated_at: archive::dated_at(self.taken_at, self.modified_ns),
        }
    }

    fn stored_preview(&self) -> StoredPreview {
        let image = self
            .image_width
            .zip(self.image_height)
            .and_then(|(width, height)| {
                Some(thumbnails::ImageSize {
                    width: u32::try_from(width).ok()?,
                    height: u32::try_from(height).ok()?,
                })
            });
        StoredPreview {
            read: self.file_size.is_some(),
            image,
            is_text: self.is_text,
        }
    }
}

/// スキャンで読んで残した、行のプレビューの情報。
#[derive(Clone, Copy)]
pub(super) struct StoredPreview {
    /// 読んだことがあるか。無ければ `image`・`is_text` は意味を持たない。
    pub(super) read: bool,
    pub(super) image: Option<thumbnails::ImageSize>,
    pub(super) is_text: bool,
}

/// 軸1本の、名前と、アイテムに現れた値。値は表示名ごとに1つにまとめて持ち、アイテムからは番号で指す。
/// 数十万件のアイテムが同じ値の文字列をそれぞれ持つと、メモリが件数に比例して膨らむため。
pub(super) struct IndexAxis {
    pub(super) name: String,
    pub(super) filterable: bool,
    values: Vec<DerivedAxisValue>,
}

/// 導出済みのアイテム1件。
pub(super) struct IndexedItem {
    pub(super) id: i64,
    pub(super) rel_path: String,
    created_at: String,
    pub(super) title: String,
    /// タイトルをテンプレートで組み立てたか (→ `archive::DerivedItem::title_from_template`)。
    title_from_template: bool,
    /// 軸ごとの値 (`IndexAxis::values` の番号)。`None` は未設定。
    values: Box<[Option<u32>]>,
    pub(super) preview: StoredPreview,
}

impl IndexedItem {
    /// `rel_path` の最後の要素。
    pub(super) fn file_name(&self) -> &str {
        self.rel_path.rsplit('/').next().unwrap_or(&self.rel_path)
    }
}

/// 1つのフォルダーの、導出済みの全アイテム。
pub(super) struct ArchiveIndex {
    pub(super) axes: Vec<IndexAxis>,
    template_axes: HashSet<String>,
    pub(super) items: Vec<IndexedItem>,
    by_id: HashMap<i64, usize>,
    /// 並び順ごとの、全アイテムの並び (`items` の添字)。絞り込んでも並びは変わらないので、
    /// 一度並べておけば、要求のたびに数十万件を並べ直さずに済む。要るときに作る。
    orders: [OnceLock<Vec<usize>>; 3],
}

impl ArchiveIndex {
    /// 索引の行を導出する。導出は全アイテム × 軸 × 照合語の数だけ回るので、`run_blocking` の中で呼ぶ。
    pub(super) fn build(rows: Vec<ItemRow>, axes: &[AxisIndex], template: Option<&str>) -> Self {
        let mut index_axes: Vec<IndexAxis> = axes
            .iter()
            .map(|axis| IndexAxis {
                name: axis.name.clone(),
                filterable: axis.filterable,
                values: Vec::new(),
            })
            .collect();
        let mut interned: Vec<HashMap<String, u32>> = vec![HashMap::new(); axes.len()];
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let derived = archive::derive_item(row.facts(), axes, template);
            let values = derived
                .axis_values
                .into_iter()
                .enumerate()
                .map(|(i, value)| {
                    let value = value?;
                    let axis = &mut index_axes[i];
                    let id = *interned[i].entry(value.display.clone()).or_insert_with(|| {
                        axis.values.push(value);
                        u32::try_from(axis.values.len() - 1).expect("axis values fit in u32")
                    });
                    Some(id)
                })
                .collect();
            items.push(IndexedItem {
                id: row.id,
                preview: row.stored_preview(),
                rel_path: row.rel_path,
                created_at: row.created_at,
                title: derived.title,
                title_from_template: derived.title_from_template,
                values,
            });
        }
        let by_id = items
            .iter()
            .enumerate()
            .map(|(index, item)| (item.id, index))
            .collect();
        Self {
            axes: index_axes,
            template_axes: archive::template_placeholder_names(template)
                .into_iter()
                .map(str::to_string)
                .collect(),
            items,
            by_id,
            orders: Default::default(),
        }
    }

    pub(super) fn get(&self, id: i64) -> Option<&IndexedItem> {
        self.by_id.get(&id).map(|&index| &self.items[index])
    }

    /// `item` の、`axis` 本目の軸の値。
    pub(super) fn value(&self, item: &IndexedItem, axis: usize) -> Option<&DerivedAxisValue> {
        item.values[axis].map(|id| &self.axes[axis].values[id as usize])
    }

    /// 軸の値の比較キーでの比較 (→ docs/archive.md「軸の値の辞書と導出」)。未設定は最後。
    fn axis_cmp(&self, a: &IndexedItem, b: &IndexedItem) -> Ordering {
        for axis in 0..self.axes.len() {
            let left = self.value(a, axis).map(|value| &value.key);
            let right = self.value(b, axis).map(|value| &value.key);
            let ordering = match (left, right) {
                (Some(left), Some(right)) => left.cmp(right),
                (Some(left), None) => left.cmp(&ValueKey::Unset),
                (None, Some(right)) => ValueKey::Unset.cmp(right),
                (None, None) => Ordering::Equal,
            };
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        Ordering::Equal
    }

    /// 並び順での、全アイテムの並び (→ docs/archive.md「エンドポイント一覧」)。どれも `rel_path` で決着させる。
    fn order(&self, order: ArchiveSortOrder) -> &[usize] {
        let slot = match order {
            ArchiveSortOrder::Axis => 0,
            ArchiveSortOrder::Title => 1,
            ArchiveSortOrder::New => 2,
        };
        self.orders[slot].get_or_init(|| {
            let mut indexes: Vec<usize> = (0..self.items.len()).collect();
            match order {
                ArchiveSortOrder::Axis => indexes.sort_by(|&a, &b| {
                    let (a, b) = (&self.items[a], &self.items[b]);
                    self.axis_cmp(a, b)
                        .then_with(|| a.rel_path.cmp(&b.rel_path))
                }),
                ArchiveSortOrder::Title => indexes.sort_by(|&a, &b| {
                    let (a, b) = (&self.items[a], &self.items[b]);
                    title_cmp(&a.title, &b.title).then_with(|| a.rel_path.cmp(&b.rel_path))
                }),
                // `created_at` はゼロ埋めした固定長の UTC 文字列なので、辞書順が日時の順になる
                // (`sort::sort_rows` の `New` と同じ考え方)。
                ArchiveSortOrder::New => indexes.sort_by(|&a, &b| {
                    let (a, b) = (&self.items[a], &self.items[b]);
                    b.created_at
                        .cmp(&a.created_at)
                        .then_with(|| a.rel_path.cmp(&b.rel_path))
                }),
            }
            indexes
        })
    }

    /// 行の2段目に出す軸の値 (→ docs/ui.md「アーカイブの一覧画面」)。表示タイトルで使っていない軸のうち、
    /// 絞り込み中でない軸の値を、軸の並び順に空白で区切る。出す値が無ければ `None`。
    pub(super) fn subtitle(
        &self,
        item: &IndexedItem,
        active_filters: &[Option<String>],
    ) -> Option<String> {
        let parts: Vec<&str> = (0..self.axes.len())
            .filter(|&axis| {
                active_filters.get(axis).is_none_or(Option::is_none)
                    && !(item.title_from_template
                        && self.template_axes.contains(&self.axes[axis].name))
            })
            .filter_map(|axis| Some(self.value(item, axis)?.display.as_str()))
            .collect();
        (!parts.is_empty()).then(|| parts.join(" "))
    }
}

/// 管理のタブの、公開の状態での絞り込み。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum StatusFilter {
    #[default]
    All,
    Published,
    Unpublished,
}

impl StatusFilter {
    /// クエリの値から決める。知らない値はすべてとして扱う (並び順と同じく、422 にしない)。
    pub(super) fn from_query(value: Option<&str>) -> Self {
        match value {
            Some("published") => Self::Published,
            Some("unpublished") => Self::Unpublished,
            _ => Self::All,
        }
    }

    fn accepts(self, published: bool) -> bool {
        match self {
            Self::All => true,
            Self::Published => published,
            Self::Unpublished => !published,
        }
    }
}

/// 一覧の絞り込みと並べ方。
pub(super) struct ListQuery {
    /// 軸の名前 → 選択肢の表示名 (→ docs/archive.md「エンドポイント一覧」)。
    pub(super) filters: HashMap<String, String>,
    /// 名前の絞り込みの語 (`list_filter::parse_terms`)。
    pub(super) terms: Vec<Vec<char>>,
    /// 名前の絞り込みでパスも照らすか (管理のタブ。→ docs/archive.md「アイテムの公開」)。
    pub(super) match_paths: bool,
    pub(super) status: StatusFilter,
    pub(super) order: ArchiveSortOrder,
}

/// 軸1本の選択肢と、それぞれを選んだときに残る件数。
pub(super) struct AxisOptions {
    pub(super) name: String,
    pub(super) options: Vec<(String, usize)>,
}

/// 公開の状態ごとの件数 (管理のタブの絞り込みの欄に出す)。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct StatusCounts {
    pub(super) all: usize,
    pub(super) published: usize,
    pub(super) unpublished: usize,
}

/// 絞って並べた結果。
pub(super) struct ListResult {
    pub(super) axes: Vec<AxisOptions>,
    /// 軸ごとに効いた絞り込み (`axes` と同じ順でなく、索引の軸と同じ順)。行の2段目を組み立てるのに使う。
    pub(super) active_filters: Vec<Option<String>>,
    /// 絞って並べたアイテムの番号。
    pub(super) ids: Vec<i64>,
    /// 軸で絞った後、名前の語と公開の状態で絞る前の数。名前で絞り込み中の「N / M 件」の M にする。
    pub(super) total_all: usize,
    pub(super) status_counts: StatusCounts,
}

/// 見えるアイテムを絞って並べ、軸の選択肢と件数を数える。数十万件を回すので `run_blocking` の中で呼ぶ。
///
/// - 選択肢は絞り込む前の、見えるアイテム全体から作る。見えるアイテムに値が1つも無い軸は返さない
///   (→ docs/archive.md「軸の値の辞書と導出」)。
/// - 軸に一致しないキー・どの選択肢にも一致しない値は無視する (→ docs/archive.md「エンドポイント一覧」)。
/// - 選択肢の件数は、ほかの軸・名前の語・公開の状態の絞り込みを効かせたまま、その軸だけを置き換えて数える。
pub(super) fn run_query(
    index: &ArchiveIndex,
    is_published: &dyn Fn(&IndexedItem) -> bool,
    published_only: bool,
    query: &ListQuery,
) -> ListResult {
    let visible: Vec<usize> = index
        .order(query.order)
        .iter()
        .copied()
        .filter(|&i| !published_only || is_published(&index.items[i]))
        .collect();

    // 軸ごとの選択肢 (表示名 → 比較キー)。絞り込みに出さない軸と、値の無い軸は `None`。
    let options_by_axis: Vec<Option<HashMap<&str, &ValueKey>>> = (0..index.axes.len())
        .map(|axis| {
            index.axes[axis]
                .filterable
                .then(|| {
                    let mut options = HashMap::new();
                    for &i in &visible {
                        if let Some(value) = index.value(&index.items[i], axis) {
                            options.entry(value.display.as_str()).or_insert(&value.key);
                        }
                    }
                    options
                })
                .filter(|options| !options.is_empty())
        })
        .collect();

    let active_filters: Vec<Option<String>> = index
        .axes
        .iter()
        .zip(&options_by_axis)
        .map(|(axis, options)| {
            let expected = query.filters.get(&axis.name)?;
            options
                .as_ref()?
                .contains_key(expected.as_str())
                .then(|| expected.clone())
        })
        .collect();

    let matches_axes = |item: &IndexedItem, skip_axis: Option<usize>| {
        active_filters.iter().enumerate().all(|(axis, expected)| {
            skip_axis == Some(axis)
                || expected.as_deref().is_none_or(|expected| {
                    index
                        .value(item, axis)
                        .is_some_and(|value| value.display == expected)
                })
        })
    };
    let matches_name = |item: &IndexedItem| {
        query.terms.is_empty()
            || super::list_filter::matches(&item.title, &query.terms)
            || (query.match_paths && super::list_filter::contains(&item.rel_path, &query.terms))
    };

    // 名前の語は軸の数より重いので、1件につき1回だけ照らしておく。
    let named: Vec<(usize, bool)> = visible
        .iter()
        .filter(|&&i| matches_name(&index.items[i]))
        .map(|&i| (i, is_published(&index.items[i])))
        .collect();

    let axes = index
        .axes
        .iter()
        .zip(options_by_axis.iter())
        .enumerate()
        .filter_map(|(axis, (index_axis, options))| {
            let options = options.as_ref()?;
            let mut counts: HashMap<&str, usize> = HashMap::new();
            for &(i, published) in &named {
                let item = &index.items[i];
                if query.status.accepts(published)
                    && matches_axes(item, Some(axis))
                    && let Some(value) = index.value(item, axis)
                {
                    *counts.entry(value.display.as_str()).or_default() += 1;
                }
            }
            // 表示名が違えば比較キーも違うので、キーだけで順が決まる。
            let mut options: Vec<(&str, &ValueKey)> =
                options.iter().map(|(name, key)| (*name, *key)).collect();
            options.sort_by(|a, b| a.1.cmp(b.1));
            Some(AxisOptions {
                name: index_axis.name.clone(),
                options: options
                    .into_iter()
                    .map(|(value, _)| (value.to_string(), counts.get(value).copied().unwrap_or(0)))
                    .collect(),
            })
        })
        .collect();

    let total_all = visible
        .iter()
        .filter(|&&i| matches_axes(&index.items[i], None))
        .count();
    let mut status_counts = StatusCounts::default();
    let mut ids = Vec::new();
    for &(i, published) in &named {
        let item = &index.items[i];
        if !matches_axes(item, None) {
            continue;
        }
        status_counts.all += 1;
        if published {
            status_counts.published += 1;
        } else {
            status_counts.unpublished += 1;
        }
        if query.status.accepts(published) {
            ids.push(item.id);
        }
    }

    ListResult {
        axes,
        active_filters,
        ids,
        total_all,
        status_counts,
    }
}

/// 結果の集まりを作った口。管理用の結果には未公開も入っているので、閲覧用の口からは引かせない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SnapshotKind {
    View,
    Manage,
}

/// 区切って引く、絞って並べた結果 (→ docs/archive.md「一覧の API を区切って返す」)。
pub(super) struct Snapshot {
    pub(super) archive_id: i64,
    pub(super) kind: SnapshotKind,
    /// 作った人。匿名の閲覧者は `None`。
    pub(super) owner: Option<i64>,
    pub(super) ids: Arc<[i64]>,
    /// 混ぜた結果 (`shuffled_audio`) だけが持つ、元の結果での各行の位置。シャッフルを切ったとき、
    /// 画面が今の曲の次から元の並びで続けるのに使う。
    pub(super) positions: Option<Arc<[u32]>>,
    pub(super) active_filters: Arc<[Option<String>]>,
    /// 作ったときの版。新しくなったかを画面に知らせる (`stale`)。
    pub(super) versions: Versions,
}

impl Snapshot {
    /// この結果のうち音声の行だけを、`seed` で混ぜた順に並べた結果 (→ docs/archive.md「音声のシャッフル」)。
    /// 区切って取る一覧は全部が画面に無いので、全体から混ぜるにはサーバーで混ぜる。
    /// 作った口・人・版はこの結果のものを引き継ぐ。
    pub(super) fn shuffled_audio(&self, index: &ArchiveIndex, seed: u64) -> Snapshot {
        let mut rows: Vec<(i64, u32)> = self
            .ids
            .iter()
            .enumerate()
            .filter(|&(_, &id)| {
                index
                    .get(id)
                    .is_some_and(|item| is_audio_file_name(item.file_name()))
            })
            // usize の半分の量で持つ。40億件の結果はメモリに載らないので、u32 に収まる。
            .map(|(position, &id)| {
                (
                    id,
                    u32::try_from(position).expect("result positions fit in u32"),
                )
            })
            .collect();
        shuffle(&mut rows, seed);
        Snapshot {
            archive_id: self.archive_id,
            kind: self.kind,
            owner: self.owner,
            ids: rows.iter().map(|&(id, _)| id).collect(),
            positions: Some(rows.iter().map(|&(_, position)| position).collect()),
            active_filters: Arc::clone(&self.active_filters),
            versions: self.versions,
        }
    }
}

/// ページ内で鳴らす音声の拡張子。画面と同じ一覧 (`frontend/src/lib/audio-extensions.json`) で、テストで揃える。
/// 画面が音声として並べる行と、混ぜる行を同じにするため。
const AUDIO_EXTENSIONS: [&str; 9] = [
    "mp3", "m4a", "wav", "ogg", "oga", "flac", "aac", "wma", "opus",
];

fn is_audio_file_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, extension)| {
        AUDIO_EXTENSIONS
            .iter()
            .any(|audio| extension.eq_ignore_ascii_case(audio))
    })
}

/// Fisher–Yates で混ぜる。乱数は splitmix64。混ぜ方に暗号の強さは要らず、この用途だけのために
/// `rand` クレートを増やさないため。
fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut state = seed;
    for i in (1..items.len()).rev() {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        // 0..=i へ偏りを小さく写す (Lemire の掛け算)。上位の64ビットは i + 1 未満なので usize に収まる。
        let bound = u128::try_from(i + 1).expect("index fits in u128");
        let j = usize::try_from((u128::from(z) * bound) >> 64).expect("below the bound");
        items.swap(i, j);
    }
}

struct StoredSnapshot {
    snapshot: Arc<Snapshot>,
    last_used: Instant,
}

#[derive(Default)]
struct Snapshots {
    map: HashMap<String, StoredSnapshot>,
    bytes: usize,
}

impl Snapshots {
    fn remove(&mut self, token: &str) {
        if let Some(stored) = self.map.remove(token) {
            self.bytes -= snapshot_bytes(&stored.snapshot);
        }
    }

    /// 使われなくなったものと、量の上限を超えた分を、古いものから捨てる。
    fn evict(&mut self, now: Instant) {
        let idle: Vec<String> = self
            .map
            .iter()
            .filter(|(_, stored)| now.duration_since(stored.last_used) > IDLE_TTL)
            .map(|(token, _)| token.clone())
            .collect();
        for token in idle {
            self.remove(&token);
        }
        while self.bytes > MAX_SNAPSHOT_BYTES {
            let Some(oldest) = self
                .map
                .iter()
                .min_by_key(|(_, stored)| stored.last_used)
                .map(|(token, _)| token.clone())
            else {
                break;
            };
            self.remove(&oldest);
        }
    }
}

fn snapshot_bytes(snapshot: &Snapshot) -> usize {
    SNAPSHOT_OVERHEAD_BYTES
        + snapshot.ids.len() * std::mem::size_of::<i64>()
        + snapshot
            .positions
            .as_ref()
            .map_or(0, |positions| positions.len() * std::mem::size_of::<u32>())
}

/// フォルダーの索引と印の版 (`archive_versions`)。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Versions {
    pub(super) index: i64,
    pub(super) marks: i64,
}

/// 今の版を読む。行が無ければ (まだ何も書いていない) 0。
pub(super) async fn versions(pool: &sqlx::SqlitePool, id: i64) -> Result<Versions, AppError> {
    let row = sqlx::query!(
        "SELECT index_version, marks_version FROM archive_versions WHERE content_id = ?",
        id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map_or_else(Versions::default, |row| Versions {
        index: row.index_version,
        marks: row.marks_version,
    }))
}

struct Entry {
    index: Option<(i64, Arc<ArchiveIndex>)>,
    /// 同じフォルダーを同時に作り始めないための鍵。冷えたところへ要求が重なると、それぞれ数秒の導出を回すため。
    build: Arc<tokio::sync::Mutex<()>>,
    last_used: Instant,
}

impl Default for Entry {
    fn default() -> Self {
        Self {
            index: None,
            build: Arc::default(),
            last_used: Instant::now(),
        }
    }
}

/// 導出の結果と印と結果の集まりの置き場。`AppState` が1つ持つ。
///
/// `std::sync::Mutex` を使うのは `ArchiveScans` と同じく、保持するのが表を触る一瞬だけで、持ったまま await しないため。
#[derive(Default)]
pub struct ArchiveIndexCache {
    entries: StdMutex<HashMap<i64, Entry>>,
    /// 印は導出の結果と別に持つ。管理画面の件数や検索のように多くのフォルダーの印を引く所が、
    /// 数を縛った導出の結果 (`MAX_ENTRIES`) を押し出さないため。印はフォルダーの数だけで、使われなくなったら捨てる。
    marks: StdMutex<HashMap<i64, StoredMarks>>,
    snapshots: StdMutex<Snapshots>,
}

struct StoredMarks {
    version: i64,
    marks: Arc<Marks>,
    last_used: Instant,
}

impl ArchiveIndexCache {
    /// 全部捨てる。バックアップから戻したときに呼ぶ (DB が丸ごと替わり、版の数も当てにならないため)。
    pub fn clear(&self) {
        self.entries
            .lock()
            .expect("archive index lock poisoned")
            .clear();
        self.marks.lock().expect("marks lock poisoned").clear();
        *self.snapshots.lock().expect("snapshot lock poisoned") = Snapshots::default();
    }

    /// `id` の表の行に触れ、作り始めるときの鍵を返す。ついでに使われなくなったものを捨てる。
    fn touch(&self, id: i64) -> Arc<tokio::sync::Mutex<()>> {
        let now = Instant::now();
        let mut entries = self.entries.lock().expect("archive index lock poisoned");
        entries.retain(|key, entry| *key == id || now.duration_since(entry.last_used) <= IDLE_TTL);
        while entries.len() >= MAX_ENTRIES && !entries.contains_key(&id) {
            let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| *key)
            else {
                break;
            };
            entries.remove(&oldest);
        }
        let entry = entries.entry(id).or_default();
        entry.last_used = now;
        Arc::clone(&entry.build)
    }

    fn cached_index(&self, id: i64, version: i64) -> Option<Arc<ArchiveIndex>> {
        let entries = self.entries.lock().expect("archive index lock poisoned");
        let (cached, index) = entries.get(&id)?.index.as_ref()?;
        (*cached == version).then(|| Arc::clone(index))
    }

    fn cached_marks(&self, id: i64, version: i64) -> Option<Arc<Marks>> {
        let now = Instant::now();
        let mut marks = self.marks.lock().expect("marks lock poisoned");
        marks.retain(|key, stored| *key == id || now.duration_since(stored.last_used) <= IDLE_TTL);
        let stored = marks
            .get_mut(&id)
            .filter(|stored| stored.version == version)?;
        stored.last_used = now;
        Some(Arc::clone(&stored.marks))
    }

    fn store_index(&self, id: i64, version: i64, index: &Arc<ArchiveIndex>) {
        let mut entries = self.entries.lock().expect("archive index lock poisoned");
        if let Some(entry) = entries.get_mut(&id) {
            entry.index = Some((version, Arc::clone(index)));
        }
    }

    fn store_marks(&self, id: i64, version: i64, marks: &Arc<Marks>) {
        self.marks.lock().expect("marks lock poisoned").insert(
            id,
            StoredMarks {
                version,
                marks: Arc::clone(marks),
                last_used: Instant::now(),
            },
        );
    }

    /// 結果の集まりを持ち、名前を返す。
    pub(super) fn create_snapshot(&self, snapshot: Snapshot) -> Result<String, AppError> {
        let token = super::random_hex::<16>()?;
        let now = Instant::now();
        let mut snapshots = self.snapshots.lock().expect("snapshot lock poisoned");
        snapshots.bytes += snapshot_bytes(&snapshot);
        snapshots.map.insert(
            token.clone(),
            StoredSnapshot {
                snapshot: Arc::new(snapshot),
                last_used: now,
            },
        );
        snapshots.evict(now);
        Ok(token)
    }

    /// 結果の集まりを引く。捨てられた・別のフォルダーや口や人のものなら `None`。
    pub(super) fn snapshot(
        &self,
        token: &str,
        archive_id: i64,
        kind: SnapshotKind,
        owner: Option<i64>,
    ) -> Option<Arc<Snapshot>> {
        let now = Instant::now();
        let mut snapshots = self.snapshots.lock().expect("snapshot lock poisoned");
        snapshots.evict(now);
        let stored = snapshots.map.get_mut(token)?;
        let snapshot = &stored.snapshot;
        if snapshot.archive_id != archive_id || snapshot.kind != kind || snapshot.owner != owner {
            return None;
        }
        stored.last_used = now;
        Some(Arc::clone(snapshot))
    }
}

/// `id` の導出済みの全アイテムを、今の版で返す。版が変わっていれば作り直す。
///
/// 版を先に読んでから材料を読むので、持つ結果は読んだ版より新しいことはあっても古いことはない。
/// 作る間に材料が変わっても、次の要求で版の違いに気づいて作り直す。
pub(super) async fn index(
    state: &AppState,
    id: i64,
) -> Result<(Versions, Arc<ArchiveIndex>), AppError> {
    let cache = &state.archive_index;
    let build = cache.touch(id);
    let current = versions(&state.pool, id).await?;
    if let Some(index) = cache.cached_index(id, current.index) {
        return Ok((current, index));
    }
    let _building = build.lock().await;
    // 待つ間に、ほかの要求が作り終えていることがある。
    let current = versions(&state.pool, id).await?;
    if let Some(index) = cache.cached_index(id, current.index) {
        return Ok((current, index));
    }
    let index = Arc::new(build_index(&state.pool, id).await?);
    cache.store_index(id, current.index, &index);
    Ok((current, index))
}

/// `id` の導出済みの全アイテム。今の版を持っていればそれを使い、無ければ作るが持たない。
/// ヘッダーの検索のように、多くのフォルダーを一度に見るところで使う。持つと、開いている一覧の分を追い出すため。
pub(super) async fn index_if_cached_or_build(
    state: &AppState,
    id: i64,
) -> Result<Arc<ArchiveIndex>, AppError> {
    let current = versions(&state.pool, id).await?;
    if let Some(index) = state.archive_index.cached_index(id, current.index) {
        return Ok(index);
    }
    Ok(Arc::new(build_index(&state.pool, id).await?))
}

/// `id` (フォルダー) の公開・非公開の印を、今の版で返す。
pub(super) async fn marks(state: &AppState, id: i64) -> Result<(i64, Arc<Marks>), AppError> {
    let cache = &state.archive_index;
    let version = versions(&state.pool, id).await?.marks;
    if let Some(marks) = cache.cached_marks(id, version) {
        return Ok((version, marks));
    }
    let marks = Arc::new(Marks::load(&state.pool, id).await?);
    cache.store_marks(id, version, &marks);
    Ok((version, marks))
}

async fn build_index(pool: &sqlx::SqlitePool, id: i64) -> Result<ArchiveIndex, AppError> {
    let template = sqlx::query_scalar!("SELECT title_template FROM contents WHERE id = ?", id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)?;
    let axes = archive::load_axis_index(pool, id).await?;
    let rows = sqlx::query_as!(
        ItemRow,
        r#"SELECT id as "id!", rel_path, created_at as "created_at!", file_size, modified_ns,
                  image_width, image_height, is_text as "is_text: bool", taken_at
           FROM archive_items WHERE archive_id = ?"#,
        id
    )
    .fetch_all(pool)
    .await?;
    run_blocking(move || ArchiveIndex::build(rows, &axes, template.as_deref())).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::archive::{AxisDict, AxisDictRow};
    use crate::api::archive_axes::{AxisMatchPosition, AxisSource};

    fn subject_axis() -> AxisIndex {
        let rows = [("kokugo", "国語"), ("eigo", "英語")]
            .iter()
            .zip(0..)
            .map(|((raw, display), position)| AxisDictRow {
                raw_value: raw.to_string(),
                display_name: Some(display.to_string()),
                position,
                match_position: AxisMatchPosition::Anywhere,
            })
            .collect();
        let dict =
            AxisDict::new(AxisSource::FilenameWord, None, rows).expect("軸の定義が正しくない");
        AxisIndex::new("科目".to_string(), true, false, dict)
    }

    fn index(rel_paths: &[&str]) -> ArchiveIndex {
        let rows = rel_paths
            .iter()
            .zip(1..)
            .map(|(rel_path, id)| ItemRow {
                id,
                rel_path: rel_path.to_string(),
                created_at: "2026-09-01T00:00:00.000Z".to_string(),
                ..ItemRow::default()
            })
            .collect();
        ArchiveIndex::build(rows, &[subject_axis()], None)
    }

    fn query(filter: &str, status: StatusFilter) -> ListQuery {
        ListQuery {
            filters: HashMap::new(),
            terms: crate::api::list_filter::parse_terms(filter),
            match_paths: true,
            status,
            order: ArchiveSortOrder::Axis,
        }
    }

    const FILES: &[&str] = &["a/kokugo_1.pdf", "a/kokugo_2.pdf", "b/eigo_1.pdf"];

    /// 名前の語と公開の状態で絞り、選択肢の件数と公開の状態ごとの件数もその絞り込みを効かせて数える。
    /// 選択肢そのものは絞り込む前の全体から作る (→ docs/archive.md「軸の値の辞書と導出」)。
    #[test]
    fn run_query_counts_options_and_statuses_within_the_name_filter() {
        let index = index(FILES);
        let published = |item: &IndexedItem| item.rel_path != "a/kokugo_2.pdf";

        let result = run_query(&index, &published, false, &query("_1", StatusFilter::All));
        assert_eq!(result.ids, [1, 3]);
        assert_eq!(result.total_all, 3);
        assert_eq!(
            result.axes[0].options,
            [("国語".to_string(), 1), ("英語".to_string(), 1)]
        );

        let result = run_query(
            &index,
            &published,
            false,
            &query("kokugo", StatusFilter::Unpublished),
        );
        assert_eq!(result.ids, [2]);
        assert_eq!(
            result.status_counts,
            StatusCounts {
                all: 2,
                published: 1,
                unpublished: 1
            }
        );
    }

    /// 管理用はパスでも照らす (続いた文字列だけ)。閲覧用は公開しているものだけを並べ、選択肢もそこから作る。
    #[test]
    fn run_query_matches_paths_only_for_manage_and_hides_unpublished_for_view() {
        let index = index(FILES);
        let published = |item: &IndexedItem| item.rel_path.starts_with("a/");

        let result = run_query(&index, &published, false, &query("b/", StatusFilter::All));
        assert_eq!(result.ids, [3]);

        let view_query = ListQuery {
            match_paths: false,
            ..query("", StatusFilter::All)
        };
        let result = run_query(&index, &published, true, &view_query);
        assert_eq!(result.ids, [1, 2]);
        assert_eq!(result.total_all, 2);
        assert_eq!(result.axes[0].options, [("国語".to_string(), 2)]);
    }

    /// 「N / M 件」の M は、軸で絞った後、名前の語で絞る前の数。
    #[test]
    fn run_query_counts_total_all_within_the_axis_filter() {
        let index = index(FILES);
        let published = |_: &IndexedItem| true;
        let axis_query = ListQuery {
            filters: HashMap::from([("科目".to_string(), "国語".to_string())]),
            ..query("_1", StatusFilter::All)
        };
        let result = run_query(&index, &published, false, &axis_query);
        assert_eq!(result.ids, [1]);
        assert_eq!(result.total_all, 2);
    }

    /// 混ぜた結果は元の結果の音声の行だけを持ち、各行の元の位置を添える。同じ種なら同じ順になる。
    #[test]
    fn shuffled_audio_keeps_only_audio_rows_with_their_positions() {
        let paths: Vec<String> = (0..40)
            .map(|n| {
                let extension = if n % 4 == 0 { "pdf" } else { "MP3" };
                format!("a/kokugo_{n}.{extension}")
            })
            .collect();
        let index = index(&paths.iter().map(String::as_str).collect::<Vec<_>>());
        let ids: Vec<i64> = index.items.iter().rev().map(|item| item.id).collect();
        let snapshot = Snapshot {
            archive_id: 1,
            kind: SnapshotKind::View,
            owner: None,
            ids: ids.clone().into(),
            positions: None,
            active_filters: Vec::new().into(),
            versions: Versions::default(),
        };

        let shuffled = snapshot.shuffled_audio(&index, 42);
        let positions = shuffled
            .positions
            .as_deref()
            .expect("混ぜた結果は位置を持つはず");
        assert_eq!(shuffled.ids.len(), 30);
        for (id, &position) in shuffled.ids.iter().zip(positions) {
            assert_eq!(ids[position as usize], *id);
            let item = index.get(*id).expect("索引にある行のはず");
            assert!(is_audio_file_name(item.file_name()));
        }
        let mut sorted = positions.to_vec();
        sorted.sort_unstable();
        assert_ne!(positions, sorted.as_slice(), "混ざっているはず");
        assert_eq!(shuffled.ids, snapshot.shuffled_audio(&index, 42).ids);
        assert_ne!(shuffled.ids, snapshot.shuffled_audio(&index, 43).ids);
    }

    /// 音声の拡張子は、画面と同じ一覧にする。
    #[test]
    fn audio_extensions_match_the_frontend() {
        let frontend: Vec<String> =
            serde_json::from_str(include_str!("../../frontend/src/lib/audio-extensions.json"))
                .expect("拡張子の一覧を読めるはず");
        assert_eq!(frontend, AUDIO_EXTENSIONS);
    }

    /// 結果の集まりは、作ったフォルダー・口・人からしか引けない。
    #[test]
    fn snapshots_are_bound_to_the_archive_kind_and_owner() {
        let cache = ArchiveIndexCache::default();
        let token = cache
            .create_snapshot(Snapshot {
                archive_id: 1,
                kind: SnapshotKind::Manage,
                owner: Some(7),
                ids: vec![1, 2].into(),
                positions: None,
                active_filters: Vec::new().into(),
                versions: Versions::default(),
            })
            .expect("結果を持てるはず");

        assert!(
            cache
                .snapshot(&token, 1, SnapshotKind::Manage, Some(7))
                .is_some()
        );
        assert!(
            cache
                .snapshot(&token, 2, SnapshotKind::Manage, Some(7))
                .is_none()
        );
        assert!(
            cache
                .snapshot(&token, 1, SnapshotKind::View, Some(7))
                .is_none()
        );
        assert!(
            cache
                .snapshot(&token, 1, SnapshotKind::Manage, None)
                .is_none()
        );

        cache.clear();
        assert!(
            cache
                .snapshot(&token, 1, SnapshotKind::Manage, Some(7))
                .is_none()
        );
    }
}
