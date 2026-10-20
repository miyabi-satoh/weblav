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

/// 照らす文字列の種類 (→ docs/search.md「一致のしかた」)。並びの順に重く、当たりの順を決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum FieldKind {
    /// 入っている場所の名前 (上のグループ・フォルダー・アーカイブ・途中のフォルダー)。そのまま含むときだけ当てる。
    Place,
    /// 説明・`note`・軸の値。そのまま含むときだけ当てる。
    Detail,
    /// その物自身の名前 (タイトル・ファイル名・題)。あいまいにも当てる。
    Own,
}

/// 1語の当たり方。並びの順によい。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Tier {
    /// 文字が順に含まれる (飛ばし打ち。`frizbee`)。
    Fuzzy,
    /// どこかにそのまま含む。
    Substring,
    /// 語の頭からそのまま含む。
    WordStart,
    /// 文字列の全体と同じ。
    Whole,
}

/// ADR: あいまいに当てる語の長さの下限 (文字数)。1〜2文字で飛ばし打ちを効かせると、ほとんど何にでも当たるため。
const FUZZY_MIN_CHARS: usize = 3;

/// ADR: 飛ばし打ちで、当たった文字の間に挟まってよい文字数の、語の長さに対する倍率。
/// `frizbee` は間の広さを点数で下げるだけで外さないので、長い名前に散った文字 (`mugenp` が `admin_archive_..._desktop` に当たる) を外す。
const FUZZY_MAX_GAPS_PER_CHAR: usize = 2;

/// 当たりの良さ。`Ord` は、よいものが先 (小さい) になる向き。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Score {
    /// 語ごとの当たり方 (照らした文字列の種類と当たり方) の合計。大きいほどよい。
    rank: u32,
    /// あいまいに当たった語の、`frizbee` の点数の合計。大きいほどよい (詰まって並ぶ・語の頭から当たるほど高い)。
    fuzzy: u32,
}

impl Ord for Score {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .rank
            .cmp(&self.rank)
            .then(other.fuzzy.cmp(&self.fuzzy))
    }
}

impl PartialOrd for Score {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// 当たったときの、当たりの良さと、どこで当たったか。
#[derive(Debug, Clone, Copy)]
pub(super) struct Match {
    pub(super) score: Score,
    /// 自身の名前でなく、説明 (`Detail`) で当たった語があるか。画面が説明を添えるかに使う。
    pub(super) in_detail: bool,
}

/// 1つの検索語。
#[derive(Clone)]
struct Term {
    chars: Vec<char>,
    /// あいまいに当てる照合。語が短ければ持たない。
    fuzzy: Option<frizbee::Matcher>,
}

impl Term {
    fn new(text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        let fuzzy = (chars.len() >= FUZZY_MIN_CHARS).then(|| {
            // fzf と同じく、語の文字が全部、順に含まれるものだけを当てる (打ち間違いは許さない)。
            // 大文字と小文字は `normalize` で揃え済み。`Smart` だと語の英字しだいで変わるので、決めて渡す。
            let config = frizbee::Config::default()
                .max_typos(Some(0))
                .casing(frizbee::CaseMatching::Ignore);
            frizbee::Matcher::new(text, &config)
        });
        Self { chars, fuzzy }
    }

    /// 1つの文字列に照らす。当たれば当たり方と、あいまいに当たったときの `frizbee` の点数。
    fn match_text(&mut self, text: &str, chars: &[char], fuzzy: bool) -> Option<(Tier, u32)> {
        if self.chars == chars {
            return Some((Tier::Whole, 0));
        }
        if let Some(word_start) = substring_at(&self.chars, chars) {
            return Some((
                if word_start {
                    Tier::WordStart
                } else {
                    Tier::Substring
                },
                0,
            ));
        }
        if !fuzzy {
            return None;
        }
        let found = self.fuzzy.as_mut()?.match_one_indices(text, 0)?;
        // 当たった文字の位置 (バイト) から、間に挟まった文字数を数える。
        // `indices` は当たった文字のバイトを指す (日本語では1文字に3つ) ので、どちらも文字の頭のバイトで数える。
        let first = *found.indices.iter().min()? as usize;
        let last = *found.indices.iter().max()? as usize;
        let span = text
            .char_indices()
            .filter(|(index, _)| (first..=last).contains(index))
            .count();
        let matched = found
            .indices
            .iter()
            .filter(|&&index| text.is_char_boundary(index as usize))
            .count();
        let gaps = span.saturating_sub(matched);
        (gaps <= self.chars.len() * FUZZY_MAX_GAPS_PER_CHAR)
            .then_some((Tier::Fuzzy, u32::from(found.score)))
    }
}

/// 空白で区切った検索語。どの語もどこかに当たり、少なくとも1語は場所でない文字列に当たれば当たりにする。
/// あいまいな照合は `frizbee` に任せ、照らす文字列の種類と並べ方はここで決める (→ docs/search.md「一致のしかた」)。
#[derive(Clone)]
pub(super) struct Terms(Vec<Term>);

impl Terms {
    /// 語が1つも無ければ `None`。全角の空白も区切りになるよう、揃えてから分ける。
    pub(super) fn parse(query: &str) -> Option<Self> {
        let terms: Vec<Term> = normalize(query).split_whitespace().map(Term::new).collect();
        (!terms.is_empty()).then_some(Self(terms))
    }

    /// `fields` の文字列は `normalize` 済み。当たらなければ `None`。
    /// 場所の名前だけで当たるものを外すのは、場所の語1つで、その下の物が全部当たって場所そのものが埋もれないようにするため。
    pub(super) fn score(&mut self, fields: &[(FieldKind, &str)]) -> Option<Match> {
        let chars: Vec<Vec<char>> = fields
            .iter()
            .map(|(_, text)| text.chars().collect())
            .collect();
        let mut score = Score { rank: 0, fuzzy: 0 };
        let mut in_detail = false;
        let mut beyond_place = false;
        for term in &mut self.0 {
            let mut best: Option<(FieldKind, Tier, u32)> = None;
            for ((kind, text), chars) in fields.iter().zip(&chars) {
                let Some((tier, fuzzy)) = term.match_text(text, chars, *kind == FieldKind::Own)
                else {
                    continue;
                };
                if best.is_none_or(|(best_kind, best_tier, best_fuzzy)| {
                    (*kind, tier, fuzzy) > (best_kind, best_tier, best_fuzzy)
                }) {
                    best = Some((*kind, tier, fuzzy));
                }
            }
            let (kind, tier, fuzzy) = best?;
            // 種類を当たり方より重くする。自身の名前で当たったものを、場所の名前でそのまま当たったものより先に出すため。
            score.rank += kind as u32 * 10 + tier as u32;
            score.fuzzy += fuzzy;
            in_detail |= kind == FieldKind::Detail;
            beyond_place |= kind != FieldKind::Place;
        }
        beyond_place.then_some(Match { score, in_detail })
    }
}

/// そのまま含むか。含むなら、どこかで語の頭 (先頭か、英数字でない文字の直後) から始まるか。
fn substring_at(term: &[char], text: &[char]) -> Option<bool> {
    if term.is_empty() || term.len() > text.len() {
        return None;
    }
    let mut found = None;
    for start in 0..=text.len() - term.len() {
        if text[start..start + term.len()] == *term {
            if start == 0 || !text[start - 1].is_alphanumeric() {
                return Some(true);
            }
            found = Some(false);
        }
    }
    found
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
    /// 説明で当たった語があるか。何で当たったか分かるよう、画面が説明を添える。
    pub(super) matched_in_description: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct SearchItemHit {
    pub(super) archive_id: i64,
    /// 行の2段目に、どこにあるかとして出す。
    pub(super) archive_title: String,
    /// アーカイブより上のグループ。ルートに近い順で、ルート直下なら空。場所の頭に出す。
    pub(super) ancestors: Vec<GroupAncestor>,
    pub(super) item: ArchiveViewItem,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct SearchResponse {
    /// `within` のコンテンツのタイトル。切り替えの「『…』の中」に出す。`within` が無ければ `None`。
    scope_title: Option<String>,
    /// 当たりのよい順 (同じ程度ならタイトル順)。区画ごとの上限 (100 件) で打ち切る。
    contents: Vec<SearchContentHit>,
    /// `contents` を打ち切ったか。
    contents_truncated: bool,
    /// アーカイブのファイル。当たりのよい順 (同じ程度なら表示タイトル順)。区画ごとの上限 (100 件) で打ち切る。
    items: Vec<SearchItemHit>,
    /// `items` を打ち切ったか。
    items_truncated: bool,
    /// フォルダーの中のファイルとディレクトリ。当たりのよい順 (同じ程度なら名前順)。区画ごとの上限 (100 件) で打ち切る。
    files: Vec<SearchFileHit>,
    /// `files` を打ち切ったか。
    files_truncated: bool,
    /// 大きなフォルダーを途中までしか辿っていないか (1つのフォルダーで 20,000 件まで)。
    /// その先のファイルとリンクの一覧は探していない。語を足しても広がらないので、打ち切りとは分けて伝える。
    folders_incomplete: bool,
    /// リンクの一覧のファイルの中のリンク。当たりのよい順 (同じ程度なら題の順)。区画ごとの上限 (100 件) で打ち切る。
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
        .map(|archive| folder_search::Container {
            id: archive.id,
            title: archive.title.clone(),
            ancestors: archive.ancestors.clone(),
        })
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
        archive_items::search_items(&state, found.archives, terms).await?;
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

    fn score(query: &str, fields: &[(FieldKind, &str)]) -> Option<Match> {
        let fields: Vec<(FieldKind, String)> = fields
            .iter()
            .map(|(kind, text)| (*kind, normalize(text)))
            .collect();
        let fields: Vec<(FieldKind, &str)> = fields
            .iter()
            .map(|(kind, text)| (*kind, text.as_str()))
            .collect();
        Terms::parse(query).expect("検索語がある").score(&fields)
    }

    fn own(query: &str, title: &str) -> Option<Score> {
        score(query, &[(FieldKind::Own, title)]).map(|found| found.score)
    }

    #[test]
    fn parse_returns_none_for_blank_queries() {
        assert!(Terms::parse("").is_none());
        assert!(Terms::parse(" \t").is_none());
        // 全角の空白も区切りとして扱う。
        assert!(Terms::parse("\u{3000}").is_none());
    }

    #[test]
    fn score_requires_every_term_somewhere() {
        let fields = [
            (FieldKind::Own, "リスニング 第1回"),
            (FieldKind::Detail, "2024"),
        ];
        assert!(score("2024 リスニング", &fields).is_some());
        assert!(score("2023 リスニング", &fields).is_none());
        assert!(own("2024\u{3000}リスニング", "2024 リスニング").is_some());
    }

    #[test]
    fn score_ignores_width_and_case() {
        assert!(own("２０２４", "2024年度").is_some());
        assert!(own("ﾊﾞｲｵ", "バイオ").is_some());
        assert!(own("abc", "ABC 教材").is_some());
        assert!(own("ＡＢＣ", "abc").is_some());
    }

    #[test]
    fn score_distinguishes_hiragana_from_katakana() {
        assert!(own("かき", "カキ").is_none());
    }

    #[test]
    fn score_uses_place_names_but_not_alone() {
        let fields = [
            (FieldKind::Own, "review-units.mp4"),
            (FieldKind::Place, "knowledge"),
            (FieldKind::Place, "mugenp"),
        ];
        assert!(score("mugenp review mp4", &fields).is_some());
        // 場所の語だけでは当てない。場所そのもの (mugenp のフォルダー) が、その下の物に埋もれないように。
        assert!(score("mugenp", &fields).is_none());
        // 場所の名前は、あいまいには当てない。
        assert!(score("mgnp review", &fields).is_none());
    }

    #[test]
    fn score_tells_whether_a_term_hit_the_detail() {
        let fields = [
            (FieldKind::Own, "英語"),
            (FieldKind::Detail, "リスニングの教材"),
        ];
        assert!(!score("英語", &fields).expect("当たる").in_detail);
        assert!(score("英語 教材", &fields).expect("当たる").in_detail);
    }

    #[test]
    fn score_accepts_skipped_letters_in_own_names() {
        assert!(own("rvwunit", "review-units.mp4").is_some());
        // 短い語は飛ばし打ちで当てない。
        assert!(own("ru", "review-units.mp4").is_none());
        // 順が違えば当てない。
        assert!(own("tinu", "review-units.mp4").is_none());
        // 長い名前に散らばった文字には当てない。
        assert!(own("mugenp", "admin_archive_axes_word_suggestions_desktop").is_none());
        // 日本語でも、間に挟まる文字は語の長さの2倍まで。
        assert!(own("英単語", "英語の単元の語彙").is_some());
        assert!(own("英単語", "英語のリスニングの単元の語彙").is_none());
    }

    #[test]
    fn score_does_not_accept_typos() {
        // 文字が足りない・入れ替わっているものは当てない (fzf と同じ)。
        assert!(own("reveiw", "review-units.mp4").is_none());
        assert!(own("revuew", "review-units.mp4").is_none());
        assert!(own("revew", "review-units.mp4").is_some());
    }

    #[test]
    fn score_ranks_closer_matches_first() {
        let whole = own("mugenp", "mugenp").expect("当たる");
        let word_start = own("unit", "review-units").expect("当たる");
        assert!(whole < own("mugenp", "mugenp のトップ").expect("当たる"));
        let inside = own("nit", "review-units").expect("当たる");
        let skipped = own("revew", "review-units").expect("当たる");
        assert!(word_start < inside);
        assert!(inside < skipped);
        // あいまいに当たったものどうしの順は `frizbee` の点数に任せる。
    }

    #[test]
    fn score_ranks_own_names_above_places() {
        let in_name = score(
            "spec md",
            &[(FieldKind::Own, "spec.md"), (FieldKind::Place, "docs")],
        );
        let in_place = score(
            "spec md",
            &[(FieldKind::Own, "01-login.md"), (FieldKind::Place, "spec")],
        );
        assert!(in_name.expect("当たる").score < in_place.expect("当たる").score);
    }
}
