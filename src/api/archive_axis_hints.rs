//! 軸を設定するときの手がかり。アーカイブのアイテムから、フォルダの階層ごとの値と
//! ファイル名によく出る語を集計して返す。
//!
//! どちらも `archive_items` のスキャン結果だけから計算し、何も保存しない。

use std::collections::{BTreeMap, HashMap, HashSet};

use axum::Json;
use axum::extract::{Path, State};
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AuthUser;
use crate::error::{AppError, run_blocking};
use crate::state::AppState;

/// 階層ごとに返す値の例の件数。どの階層を軸にするかの見当が付けば足りる。
const DIR_LEVEL_SAMPLE_LIMIT: usize = 5;

#[derive(Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DirLevelSample {
    value: String,
    /// この値を持つアイテム数。
    count: usize,
}

#[derive(Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DirLevelSummary {
    /// 1始まりの階層番号。フォルダの階層の軸の `dirLevel` と同じ数え方。
    level: i64,
    /// この階層に現れる、異なる値の数。
    value_count: usize,
    /// この階層を持つアイテム数。
    item_count: usize,
    /// 値の例。件数の多い順、同数なら値の昇順で最大5つ。
    samples: Vec<DirLevelSample>,
}

/// `rel_path` のディレクトリ部分を階層ごとに集計する。階層1から深い順。
fn summarize_dir_levels(rel_paths: &[String]) -> Vec<DirLevelSummary> {
    // 階層番号の数え方を軸の導出と食い違わせないため、`dir_level_value` で引く。
    let mut levels: Vec<HashMap<&str, usize>> = Vec::new();
    for rel_path in rel_paths {
        for (index, value) in (1..)
            .map_while(|level| super::archive::dir_level_value(rel_path, level))
            .enumerate()
        {
            if levels.len() <= index {
                levels.push(HashMap::new());
            }
            *levels[index].entry(value).or_default() += 1;
        }
    }

    (1..)
        .zip(levels)
        .map(|(level, counts)| {
            let mut samples: Vec<(&str, usize)> = counts.into_iter().collect();
            samples.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
            DirLevelSummary {
                level,
                value_count: samples.len(),
                item_count: samples.iter().map(|(_, count)| count).sum(),
                samples: samples
                    .into_iter()
                    .take(DIR_LEVEL_SAMPLE_LIMIT)
                    .map(|(value, count)| DirLevelSample {
                        value: value.to_string(),
                        count,
                    })
                    .collect(),
            }
        })
        .collect()
}

/// フォルダの階層ごとの値。軸の追加ダイアログで、階層番号の代わりに実際の値を見て選ばせる。
#[utoipa::path(
    get,
    path = "/contents/{id}/dir-levels",
    params(("id" = i64, Path)),
    responses(
        (status = OK, body = Vec<DirLevelSummary>, description = "階層ごとの値の集計。アイテムが無ければ空"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しない・アーカイブでない"),
    )
)]
async fn list_dir_levels(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<DirLevelSummary>>, AppError> {
    super::archive::ensure_manageable_archive(&state.pool, &user, id).await?;
    let rel_paths = super::archive::load_item_rel_paths(&state.pool, id).await?;
    // 集計はアイテム数に比例するので、非同期のワーカーを塞がない (→ docs/archive.md「軸の設定を助ける表示」)。
    let summary = run_blocking(move || summarize_dir_levels(&rel_paths)).await?;
    Ok(Json(summary))
}

/// 語の切り出しで、塊をさらに分ける単位の文字種。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharKind {
    Digit,
    Latin,
    Kanji,
    Katakana,
    Hiragana,
    Other,
}

impl CharKind {
    fn of(c: char) -> Self {
        match c {
            '0'..='9' | '０'..='９' => Self::Digit,
            'Ａ'..='Ｚ' | 'ａ'..='ｚ' => Self::Latin,
            // ラテン文字のブロック (基本・Latin-1 補助・拡張 A/B) の文字。
            // 記号 (`×` など) は英数字でないので、ここに来る前に区切りとして落ちている。
            c if c.is_alphabetic() && u32::from(c) < 0x0250 => Self::Latin,
            // 々〆〇 は漢字の語の中に現れる。
            '\u{3005}'..='\u{3007}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{20000}'..='\u{3FFFF}' => Self::Kanji,
            // 長音符 `ー` (U+30FC)・半角の `ｰ` (U+FF70) を含む。
            '\u{30A0}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{FF66}'..='\u{FF9F}' => {
                Self::Katakana
            }
            '\u{3040}'..='\u{309F}' => Self::Hiragana,
            _ => Self::Other,
        }
    }
}

/// 前の文字に付く結合文字 (分解形の濁点・半濁点・アクセントなど) か。
///
/// Mac から来たファイル名は分解形 (NFD) のことがあり、`ガ` が `カ` と濁点の2文字になる。
/// 結合文字は英数字でも文字でもないので、前の文字の続きとして扱わないと、語がそこで切れる。
fn is_combining_mark(c: char) -> bool {
    matches!(
        c,
        '\u{0300}'..='\u{036F}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE20}'..='\u{FE2F}'
            | '\u{3099}'..='\u{309A}'
    )
}

/// 文字種の変わり目で塊を分けるか。
///
/// 英字と数字が続く語 (`part4`・`unit2` など) は1つの語として扱い、その境目では分けない。
/// 英字と数字はファイル名の中で一体の識別子になることが多く、`part` と `4` に割ると
/// かえって使いにくい候補が増えるため。漢字・かなと数字の境目 (`中1` など) は分ける。
fn splits_between(previous: CharKind, current: CharKind) -> bool {
    if previous == current {
        return false;
    }
    !matches!(
        (previous, current),
        (CharKind::Latin, CharKind::Digit) | (CharKind::Digit, CharKind::Latin)
    )
}

/// 拡張子を除いたファイル名から、語の候補を切り出す。
///
/// 英数字・文字以外で区切った塊と、塊を文字種の変わり目で分けた部分の両方を返す。
/// ただし英字と数字の連続は分けない (→ `splits_between`)。塊が先、分けた部分が後。
/// 同じ綴りは最初の1つだけ残す。
fn filename_word_candidates(stem: &str) -> Vec<&str> {
    let chunks: Vec<&str> = stem
        .split(|c: char| !c.is_alphanumeric() && !is_combining_mark(c))
        .filter(|chunk| !chunk.is_empty())
        .collect();

    let mut parts = Vec::new();
    for chunk in &chunks {
        let mut start = 0;
        let mut previous: Option<CharKind> = None;
        for (index, c) in chunk.char_indices() {
            if is_combining_mark(c) {
                continue;
            }
            let kind = CharKind::of(c);
            if previous.is_some_and(|previous| splits_between(previous, kind)) {
                parts.push(&chunk[start..index]);
                start = index;
            }
            previous = Some(kind);
        }
        // 文字種が1つだけの塊は、分けた部分が塊そのものと同じになる。
        if start > 0 {
            parts.push(&chunk[start..]);
        }
    }

    let mut seen = HashSet::new();
    chunks
        .into_iter()
        .chain(parts)
        .filter(|word| seen.insert(*word))
        .collect()
}

#[derive(Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilenameWord {
    /// 表示する綴り。大文字小文字だけが違う綴りのうち、いちばん多く出たもの。
    word: String,
    /// この語を含むアイテム数。
    count: usize,
    /// 照合語の候補として既定では隠すか。数字だけ・1文字・1件だけ・全アイテムに出る語。
    hidden_by_default: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilenameWordsResponse {
    /// アーカイブの全アイテム数。
    item_count: usize,
    /// 件数の多い順、同数なら語の昇順。
    words: Vec<FilenameWord>,
}

/// 語ごとの集計。キーは小文字化した語 (照合が大文字小文字を無視するため)。
#[derive(Default)]
struct WordTally<'a> {
    count: usize,
    /// 綴りごとの、その綴りを含むアイテム数。`BTreeMap` にして、同数なら辞書順で先の綴りを採る。
    spellings: BTreeMap<&'a str, usize>,
}

fn summarize_filename_words(rel_paths: &[String]) -> FilenameWordsResponse {
    let item_count = rel_paths.len();
    let mut tallies: HashMap<String, WordTally> = HashMap::new();
    for rel_path in rel_paths {
        let mut seen_in_item = HashSet::new();
        for word in filename_word_candidates(super::archive::file_stem(rel_path)) {
            let key = word.to_lowercase();
            let tally = tallies.entry(key.clone()).or_default();
            // 候補は綴りごとに1つなので、綴りはアイテムごとに1回だけ数えられる。
            *tally.spellings.entry(word).or_default() += 1;
            if seen_in_item.insert(key) {
                tally.count += 1;
            }
        }
    }

    let mut words: Vec<FilenameWord> = tallies
        .into_values()
        .map(|tally| {
            // `max_by_key` は同値なら後の要素を返すので、逆順に辿って辞書順で先の綴りを残す。
            let word = tally
                .spellings
                .iter()
                .rev()
                .max_by_key(|(_, count)| **count)
                .map(|(spelling, _)| spelling.to_string())
                .expect("集計に入った語は綴りを1つ以上持つ");
            let hidden_by_default = word.chars().all(|c| CharKind::of(c) == CharKind::Digit)
                || word.chars().filter(|c| !is_combining_mark(*c)).count() == 1
                || tally.count == 1
                || tally.count == item_count;
            FilenameWord {
                word,
                count: tally.count,
                hidden_by_default,
            }
        })
        .collect();
    words.sort_unstable_by(|a, b| b.count.cmp(&a.count).then_with(|| a.word.cmp(&b.word)));

    FilenameWordsResponse { item_count, words }
}

/// ファイル名によく出る語。ファイル名に含まれる語の軸で、照合語の候補として出す。
#[utoipa::path(
    get,
    path = "/contents/{id}/filename-words",
    params(("id" = i64, Path)),
    responses(
        (status = OK, body = FilenameWordsResponse, description = "語ごとの件数"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しない・アーカイブでない"),
    )
)]
async fn list_filename_words(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<FilenameWordsResponse>, AppError> {
    super::archive::ensure_manageable_archive(&state.pool, &user, id).await?;
    let rel_paths = super::archive::load_item_rel_paths(&state.pool, id).await?;
    // 語の切り出しはアイテム数に比例するので、非同期のワーカーを塞がない (→ docs/archive.md「軸の設定を助ける表示」)。
    let words = run_blocking(move || summarize_filename_words(&rel_paths)).await?;
    Ok(Json(words))
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_dir_levels))
        .routes(routes!(list_filename_words))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(rel_paths: &[&str]) -> Vec<String> {
        rel_paths.iter().map(|path| path.to_string()).collect()
    }

    fn sample(value: &str, count: usize) -> DirLevelSample {
        DirLevelSample {
            value: value.to_string(),
            count,
        }
    }

    #[test]
    fn summarize_dir_levels_counts_values_per_level_and_skips_file_names() {
        let summary = summarize_dir_levels(&paths(&[
            "2024/第1回/a.mp3",
            "2024/第2回/b.mp3",
            "2023/第1回/c.mp3",
            "2022/d.mp3",
            "e.mp3",
        ]));
        assert_eq!(
            summary,
            vec![
                DirLevelSummary {
                    level: 1,
                    value_count: 3,
                    item_count: 4,
                    samples: vec![sample("2024", 2), sample("2022", 1), sample("2023", 1)],
                },
                DirLevelSummary {
                    level: 2,
                    value_count: 2,
                    item_count: 3,
                    samples: vec![sample("第1回", 2), sample("第2回", 1)],
                },
            ]
        );
    }

    #[test]
    fn summarize_dir_levels_limits_samples_and_returns_empty_without_items() {
        let rel_paths: Vec<String> = (0..7).map(|n| format!("{n}/a.mp3")).collect();
        let summary = summarize_dir_levels(&rel_paths);
        assert_eq!(summary[0].value_count, 7);
        assert_eq!(summary[0].samples.len(), DIR_LEVEL_SAMPLE_LIMIT);

        assert!(summarize_dir_levels(&[]).is_empty());
        assert!(summarize_dir_levels(&paths(&["a.mp3"])).is_empty());
    }

    /// 分解形 (NFD) の濁点・半濁点・アクセントの位置では切らない。
    #[test]
    fn filename_word_candidates_keeps_combining_marks_with_the_previous_character() {
        let guide = "\u{30AB}\u{3099}\u{30A4}\u{30C8}\u{3099}";
        assert_eq!(
            filename_word_candidates(&format!("{guide}_2024")),
            vec![guide, "2024"]
        );
        let cafe = "cafe\u{0301}";
        assert_eq!(filename_word_candidates(cafe), vec![cafe]);
    }

    #[test]
    fn filename_word_candidates_splits_by_separators_then_by_character_kind() {
        // 英字と数字の連続は分けない (part4 を part と 4 に割らない)。漢字・かなとの境目は分ける。
        assert_eq!(filename_word_candidates("part4"), vec!["part4"]);
        assert_eq!(filename_word_candidates("P2Q-part3"), vec!["P2Q", "part3"]);
        assert_eq!(
            filename_word_candidates("2021中1_04_02英語"),
            vec!["2021中1", "04", "02英語", "2021", "中", "1", "02", "英語"]
        );
        // 括弧・中黒・全角空白・ドットも区切り。カタカナの長音符は語の一部。
        assert_eq!(
            filename_word_candidates("【リスニング】ひらがな・テスト　v1.2"),
            vec!["リスニング", "ひらがな", "テスト", "v1", "2"]
        );
        // 全角英数字も、半角と同じ文字種として扱う。連続していれば分けない。
        assert_eq!(
            filename_word_candidates("ＰＡＲＴ１２"),
            vec!["ＰＡＲＴ１２"]
        );
    }

    fn word(word: &str, count: usize, hidden_by_default: bool) -> FilenameWord {
        FilenameWord {
            word: word.to_string(),
            count,
            hidden_by_default,
        }
    }

    #[test]
    fn summarize_filename_words_merges_case_and_counts_items_once() {
        let summary = summarize_filename_words(&paths(&[
            "a/Listening_listening.mp3",
            "b/LISTENING.mp3",
            "b/Listening_script.pdf",
            "c/script_answer.pdf",
        ]));
        assert_eq!(summary.item_count, 4);
        assert_eq!(
            summary.words,
            vec![
                // 1つのファイル名に2回出ても1件。綴りは最多の `Listening`。
                word("Listening", 3, false),
                word("script", 2, false),
                word("answer", 1, true),
            ]
        );
    }

    #[test]
    fn summarize_filename_words_prefers_the_alphabetically_first_spelling_on_ties() {
        let summary = summarize_filename_words(&paths(&["kyu_x.pdf", "KYU_y.pdf", "z.pdf"]));
        assert_eq!(summary.words[0], word("KYU", 2, false));
    }

    #[test]
    fn summarize_filename_words_hides_digits_single_chars_rare_and_ubiquitous_words() {
        let summary = summarize_filename_words(&paths(&[
            "2024_過去問_英語_A.pdf",
            "2024_過去問_数学_A.pdf",
            "2023_過去問_英語.pdf",
        ]));
        let hidden: HashMap<&str, bool> = summary
            .words
            .iter()
            .map(|word| (word.word.as_str(), word.hidden_by_default))
            .collect();
        // 全アイテムに出る。
        assert!(hidden["過去問"]);
        // 数字だけ。
        assert!(hidden["2024"]);
        // 1文字。
        assert!(hidden["A"]);
        // 1件だけ。
        assert!(hidden["数学"]);
        assert!(!hidden["英語"]);
        // 全角の数字も数字だけとみなす。
        let summary =
            summarize_filename_words(&paths(&["２０２４_x.pdf", "２０２４_y.pdf", "z.pdf"]));
        assert_eq!(summary.words[0], word("２０２４", 2, true));
    }
}
