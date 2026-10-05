//! 閲覧する一覧の並び順 (→ docs/ui.md「ホーム・グループ・フォルダ・アーカイブの並び順」)。
//!
//! 並べ替えをSQLではなくここで行うのは、「第2回」を「第10回」より前に置くために
//! 数字を数値として比べる必要があるため。1つの一覧は数十件規模なので、
//! 全件をメモリ上で並べ替えてもコストは問題にならない。

use std::cmp::Ordering;

use serde::Deserialize;
use utoipa::IntoParams;

/// 閲覧する一覧の並び順を受けるクエリ。ホーム・グループ・フォルダで共有する
/// (この3つは `SortOrder` の意味論がすべて同じため)。
#[derive(Debug, Deserialize, IntoParams)]
pub(super) struct SortQuery {
    /// 並び順。`title` (既定、タイトル順) か `new` (新しい順)。
    /// 知らない値は既定として扱う (→ docs/ui.md「ホーム・グループ・フォルダ・アーカイブの並び順」)。
    #[serde(default)]
    pub(super) sort: Option<String>,
}

/// ホーム・グループ・フォルダの一覧の並び順。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum SortOrder {
    /// タイトルの昇順 (既定)。
    #[default]
    Title,
    /// 追加した日時 (フォルダはファイルの更新日時) の降順。
    New,
}

impl SortOrder {
    /// クエリの値から並び順を決める。知らない値は既定として扱い、422にはしない。
    /// 共有されたリンクを壊さないため (→ docs/ui.md「ホーム・グループ・フォルダ・アーカイブの並び順」)。
    pub(super) fn from_query(value: Option<&str>) -> Self {
        match value {
            Some("new") => Self::New,
            _ => Self::Title,
        }
    }
}

/// アーカイブの一覧の並び順。軸ベースの並びが既定な点がホーム/グループ/フォルダと違うため、
/// `SortOrder` とは別の型にする (→ docs/archive.md「エンドポイント一覧」, docs/ui.md「ホーム・グループ・フォルダ・アーカイブの並び順」)。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum ArchiveSortOrder {
    /// 軸の並び順 (既定)。
    #[default]
    Axis,
    /// タイトルの昇順。
    Title,
    /// 追加した日時 (スキャンで見つかった日時) の降順。
    New,
}

impl ArchiveSortOrder {
    /// クエリの値から並び順を決める。知らない値は既定として扱い、422にはしない
    /// (→ docs/archive.md「エンドポイント一覧」、共有されたリンクを壊さないため)。
    pub(super) fn from_query(value: Option<&str>) -> Self {
        match value {
            Some("title") => Self::Title,
            Some("new") => Self::New,
            _ => Self::Axis,
        }
    }
}

/// タイトルの比較。英字の大文字小文字は区別せず、数字の並びは数値として比べる。
///
/// 数字は `u64` 等に変換せず、先頭の0を除いた桁数→文字列の順で比べる。
/// 桁数の上限を気にしなくて済み、ファイル名由来の長い数字でも破綻しないため。
pub(super) fn title_cmp(a: &str, b: &str) -> Ordering {
    let left: Vec<char> = a.chars().collect();
    let right: Vec<char> = b.chars().collect();
    let (mut i, mut j) = (0, 0);

    while i < left.len() && j < right.len() {
        if left[i].is_ascii_digit() && right[j].is_ascii_digit() {
            let (left_digits, next_i) = digit_run(&left, i);
            let (right_digits, next_j) = digit_run(&right, j);
            match compare_digits(left_digits, right_digits) {
                Ordering::Equal => {
                    i = next_i;
                    j = next_j;
                }
                other => return other,
            }
        } else {
            // `to_lowercase` はイテレータを返すので、そのまま順に比べる
            // (全角英字やドイツ語の ß のように1文字が複数文字になる場合があるため)。
            match left[i].to_lowercase().cmp(right[j].to_lowercase()) {
                Ordering::Equal => {
                    i += 1;
                    j += 1;
                }
                other => return other,
            }
        }
    }

    // 片方が尽きたら、残りが短い方を前に置く。
    (left.len() - i).cmp(&(right.len() - j))
}

/// `from` から続く数字の並びを、先頭の0を除いて返す。第2要素は数字の次の位置。
fn digit_run(chars: &[char], from: usize) -> (&[char], usize) {
    let mut end = from;
    while end < chars.len() && chars[end].is_ascii_digit() {
        end += 1;
    }
    let mut start = from;
    // 全部が0なら最後の1桁を残す ("000" と "0" を同じ値として扱うため)。
    while start + 1 < end && chars[start] == '0' {
        start += 1;
    }
    (&chars[start..end], end)
}

/// 先頭の0を除いた数字同士を比べる。桁数が多い方が大きく、同じ桁数なら文字列の順。
fn compare_digits(a: &[char], b: &[char]) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut titles: Vec<&str>) -> Vec<&str> {
        titles.sort_by(|a, b| title_cmp(a, b));
        titles
    }

    #[test]
    fn from_query_falls_back_to_the_default() {
        assert_eq!(SortOrder::from_query(Some("new")), SortOrder::New);
        assert_eq!(SortOrder::from_query(Some("title")), SortOrder::Title);
        // 知らない値・空・未指定はすべて既定。
        assert_eq!(SortOrder::from_query(Some("updated")), SortOrder::Title);
        assert_eq!(SortOrder::from_query(Some("")), SortOrder::Title);
        assert_eq!(SortOrder::from_query(None), SortOrder::Title);
    }

    #[test]
    fn archive_sort_order_from_query_falls_back_to_axis() {
        assert_eq!(
            ArchiveSortOrder::from_query(Some("title")),
            ArchiveSortOrder::Title
        );
        assert_eq!(
            ArchiveSortOrder::from_query(Some("new")),
            ArchiveSortOrder::New
        );
        // 知らない値・空・未指定はすべて既定 (軸ベース)。
        assert_eq!(
            ArchiveSortOrder::from_query(Some("axis")),
            ArchiveSortOrder::Axis
        );
        assert_eq!(
            ArchiveSortOrder::from_query(Some("")),
            ArchiveSortOrder::Axis
        );
        assert_eq!(ArchiveSortOrder::from_query(None), ArchiveSortOrder::Axis);
    }

    #[test]
    fn title_cmp_compares_digits_as_numbers() {
        assert_eq!(
            sorted(vec!["第10回", "第2回", "第1回"]),
            vec!["第1回", "第2回", "第10回"]
        );
        // 数字が途中で終わる場合も、そこまでの数値で決まる。
        assert_eq!(sorted(vec!["a100", "a9"]), vec!["a9", "a100"]);
        // 先頭の0は値に影響しない。続きの文字で決まる。
        assert_eq!(sorted(vec!["007b", "7a"]), vec!["7a", "007b"]);
        assert_eq!(title_cmp("0", "000"), Ordering::Equal);
    }

    #[test]
    fn title_cmp_ignores_letter_case() {
        assert_eq!(title_cmp("Eiken", "eiken"), Ordering::Equal);
        assert_eq!(sorted(vec!["banana", "Apple"]), vec!["Apple", "banana"]);
        // 全角の英字も畳む。
        assert_eq!(title_cmp("ＡＢＣ", "ａｂｃ"), Ordering::Equal);
    }

    #[test]
    fn title_cmp_puts_a_prefix_first() {
        assert_eq!(
            sorted(vec!["英検 過去問", "英検"]),
            vec!["英検", "英検 過去問"]
        );
        assert_eq!(title_cmp("", ""), Ordering::Equal);
        assert_eq!(title_cmp("", "a"), Ordering::Less);
    }
}
