//! 閲覧の場の検索 (→ docs/search.md)。
//!
//! 見える範囲の判定は一覧と同じヘルパー (`can_list`) を通す。

use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::Viewer;
use crate::error::AppError;
use crate::state::AppState;

use super::archive_items::{self, ArchiveViewItem};
use super::contents::{self, ContentResponse, GroupAncestor};
use super::folder_search::{self, SearchFileHit, SearchLinkHit};

/// ADR: 区画ごとに返す件数の上限 (→ docs/search.md「結果の並びと上限」)。
/// 語を足さずに目で追える数として 100 件にしている。
pub(super) const RESULT_LIMIT: usize = 100;

/// 比べる前の揃え方。NFKC で全角と半角をまとめ、英字を小文字にする。
pub(super) fn normalize(text: &str) -> String {
    icu_normalizer::ComposingNormalizerBorrowed::new_nfkc()
        .normalize(text)
        .to_lowercase()
}

/// 空白で区切った検索語。どれもどこかに含まれていれば当たりにする。
#[derive(Clone)]
pub(super) struct Terms(Vec<String>);

impl Terms {
    /// 語が1つも無ければ `None`。全角の空白も区切りになるよう、揃えてから分ける。
    pub(super) fn parse(query: &str) -> Option<Self> {
        let terms: Vec<String> = normalize(query)
            .split_whitespace()
            .map(str::to_string)
            .collect();
        (!terms.is_empty()).then_some(Self(terms))
    }

    /// `fields` は `normalize` 済みの文字列。語ごとに、どれか1つに含まれていればよい。
    pub(super) fn matches(&self, fields: &[&str]) -> bool {
        self.0
            .iter()
            .all(|term| fields.iter().any(|field| field.contains(term.as_str())))
    }
}

#[derive(Debug, Deserialize, IntoParams)]
struct SearchQuery {
    /// 検索語。空白で区切ると、どの語も含むものに絞る。
    #[serde(default)]
    q: String,
    /// 探す場所のグループ・フォルダー・アーカイブの id。無ければ全体を探す。
    within: Option<i64>,
    /// `within` がフォルダーのとき、その中の階層 (登録パスからの相対パス)。
    #[serde(default)]
    path: String,
    /// `true` なら `within` で絞らず全体を探す。範囲の名前は返すので、画面は切り替えを出したままにできる。
    #[serde(default)]
    all: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct SearchContentHit {
    pub(super) content: ContentResponse,
    /// 上のグループ。ルートに近い順で、最後が親。ルート直下なら空。行の2段目に、どこにあるかとして出す。
    pub(super) ancestors: Vec<GroupAncestor>,
    /// タイトルでは当たらず、説明で当たったか。何で当たったか分かるよう、画面が説明を添える。
    pub(super) matched_in_description: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct SearchItemHit {
    pub(super) archive_id: i64,
    /// 行の2段目に、どこにあるかとして出す。
    pub(super) archive_title: String,
    pub(super) item: ArchiveViewItem,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct SearchResponse {
    /// `within` のコンテンツのタイトル。切り替えの「『…』の中」に出す。`within` が無ければ `None`。
    scope_title: Option<String>,
    /// タイトル順。区画ごとの上限 (100 件) で打ち切る。
    contents: Vec<SearchContentHit>,
    /// `contents` を打ち切ったか。
    contents_truncated: bool,
    /// アーカイブのファイル。表示タイトル順。区画ごとの上限 (100 件) で打ち切る。
    items: Vec<SearchItemHit>,
    /// `items` を打ち切ったか。
    items_truncated: bool,
    /// フォルダーの中のファイルとディレクトリ。名前順。区画ごとの上限 (100 件) で打ち切る。
    files: Vec<SearchFileHit>,
    /// `files` を打ち切ったか。
    files_truncated: bool,
    /// 大きなフォルダーを途中までしか辿っていないか (1つのフォルダーで 20,000 件まで)。
    /// その先のファイルとリンクの一覧は探していない。語を足しても広がらないので、打ち切りとは分けて伝える。
    folders_incomplete: bool,
    /// リンクの一覧のファイルの中のリンク。題の順。区画ごとの上限 (100 件) で打ち切る。
    links: Vec<SearchLinkHit>,
    /// `links` を打ち切ったか。
    links_truncated: bool,
}

/// 閲覧者がホームからたどって一覧で見られるコンテンツ・アーカイブの公開アイテム・フォルダーの中・
/// リンクの一覧のファイルの中のリンクを、タイトルの文字列で探す (→ docs/search.md)。語が空なら空の結果を返す。
/// `within` を渡すと、その場所の中だけを探す (→ docs/search.md「範囲」)。
#[utoipa::path(
    get,
    path = "/search",
    params(SearchQuery),
    responses(
        (status = OK, body = SearchResponse, description = "区画ごとの検索結果"),
        (status = 401, body = crate::error::ErrorResponse, description = "範囲の場所を開くにはログインが要る"),
        (status = 404, body = crate::error::ErrorResponse, description = "範囲の場所が無いか、開けないか、グループ・フォルダー・アーカイブでない"),
    )
)]
async fn search(
    viewer: Viewer,
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, AppError> {
    let scope = match query.within {
        Some(id) => Some(contents::search_scope(&state, &viewer, id, &query.path).await?),
        None => None,
    };
    let scope_title = scope.as_ref().map(|scope| scope.title.clone());
    let scope = scope.filter(|_| !query.all);
    let Some(terms) = Terms::parse(&query.q) else {
        return Ok(Json(SearchResponse {
            scope_title,
            contents: Vec::new(),
            contents_truncated: false,
            items: Vec::new(),
            items_truncated: false,
            files: Vec::new(),
            files_truncated: false,
            folders_incomplete: false,
            links: Vec::new(),
            links_truncated: false,
        }));
    };
    let found = contents::search_contents(&state, &viewer, &terms, scope.as_ref()).await?;
    let archives = found
        .archives
        .iter()
        .map(|archive| (archive.id, archive.title.clone()))
        .collect();
    let in_folders = folder_search::search(
        &state,
        &viewer,
        found.folders,
        archives,
        found.links_files,
        &terms,
    )
    .await?;
    let (items, items_truncated) =
        archive_items::search_items(&state.pool, found.archives, terms).await?;
    Ok(Json(SearchResponse {
        scope_title,
        contents: found.hits,
        contents_truncated: found.truncated,
        items,
        items_truncated,
        files: in_folders.files,
        files_truncated: in_folders.files_truncated,
        folders_incomplete: in_folders.incomplete,
        links: in_folders.links,
        links_truncated: in_folders.links_truncated,
    }))
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(search))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(query: &str, fields: &[&str]) -> bool {
        let fields: Vec<String> = fields.iter().map(|field| normalize(field)).collect();
        let fields: Vec<&str> = fields.iter().map(String::as_str).collect();
        Terms::parse(query).expect("検索語がある").matches(&fields)
    }

    #[test]
    fn parse_returns_none_for_blank_queries() {
        assert!(Terms::parse("").is_none());
        assert!(Terms::parse(" \t").is_none());
        // 全角の空白も区切りとして扱う。
        assert!(Terms::parse("\u{3000}").is_none());
    }

    #[test]
    fn matches_requires_every_term_somewhere() {
        assert!(matches("2024 リスニング", &["リスニング 第1回", "2024"]));
        assert!(!matches("2024 リスニング", &["リスニング 第1回", "2023"]));
        assert!(matches("2024\u{3000}リスニング", &["2024 リスニング"]));
    }

    #[test]
    fn matches_ignores_width_and_case() {
        assert!(matches("２０２４", &["2024年度"]));
        assert!(matches("ﾊﾞｲｵ", &["バイオ"]));
        assert!(matches("abc", &["ABC 教材"]));
        assert!(matches("ＡＢＣ", &["abc"]));
    }

    #[test]
    fn matches_distinguishes_hiragana_from_katakana() {
        assert!(!matches("かき", &["カキ"]));
    }
}
