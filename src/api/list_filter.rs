//! 一覧の名前の絞り込みの当たり方 (→ docs/ui.md「一覧の絞り込み」)。
//!
//! 画面の `$lib/list-filter.ts` と同じ規則を持つ。アーカイブの一覧は区切って返すので、
//! 手元に全件が無い画面に代わってサーバーで絞る (→ docs/archive.md「一覧の API を区切って返す」)。
//! 強調の位置は画面が出すので、ここは当たるかだけを返す。両方を `list-filter-cases.json` の同じ表でテストする。

/// 前の字にまとめて揃える印。半角カナの濁点・半濁点 (ﾊﾞ) は、前の字と合わせて初めて「バ」になるため。
const VOICING_MARKS: [char; 4] = ['\u{FF9E}', '\u{FF9F}', '\u{3099}', '\u{309A}'];

/// 空白 (全角を含む) で区切った語。揃えてから分ける。語が無ければ空 (全部に当たる)。
pub(super) fn parse_terms(query: &str) -> Vec<Vec<char>> {
    super::search::normalize(query)
        .split_whitespace()
        .map(|term| term.chars().collect())
        .collect()
}

/// 揃えた文字の並び。濁点・半濁点は前の字とまとめて揃える (画面の `normalizedUnits` と同じ)。
fn normalized_chars(text: &str) -> Vec<char> {
    let source: Vec<char> = text.chars().collect();
    let mut chars = Vec::with_capacity(source.len());
    let mut index = 0;
    while index < source.len() {
        let mut cluster = String::from(source[index]);
        while index + 1 < source.len() && VOICING_MARKS.contains(&source[index + 1]) {
            index += 1;
            cluster.push(source[index]);
        }
        chars.extend(super::search::normalize(&cluster).chars());
        index += 1;
    }
    chars
}

/// どの語も `text` に当たるか (画面の `matchFilter` が `null` でないこと)。
/// 語は、続きのまま含まれているか、その順に間を空けて現れれば当たる (fzf と同じ考え方)。
pub(super) fn matches(text: &str, terms: &[Vec<char>]) -> bool {
    if terms.is_empty() {
        return true;
    }
    let chars = normalized_chars(text);
    terms.iter().all(|term| {
        // 間を空けた当たりは続いた当たりも含むので、当たるかだけなら順に現れるかを見れば足りる。
        let mut rest = chars.iter();
        term.iter().all(|wanted| rest.any(|char| char == wanted))
    })
}

/// どの語も、続いたまま `text` に含まれるか (画面の `containsTerms`)。パスのように長い文字列で使う。
pub(super) fn contains(text: &str, terms: &[Vec<char>]) -> bool {
    let normalized = super::search::normalize(text);
    terms
        .iter()
        .all(|term| normalized.contains(&term.iter().collect::<String>()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Case {
        text: String,
        query: String,
        hit: bool,
    }

    #[derive(serde::Deserialize)]
    struct Cases {
        #[serde(rename = "match")]
        match_cases: Vec<Case>,
        contains: Vec<Case>,
    }

    fn cases() -> Cases {
        serde_json::from_str(include_str!(
            "../../frontend/src/lib/list-filter-cases.json"
        ))
        .expect("絞り込みの表を読めるはず")
    }

    #[test]
    fn matches_agrees_with_the_shared_cases() {
        for case in cases().match_cases {
            assert_eq!(
                matches(&case.text, &parse_terms(&case.query)),
                case.hit,
                "{:?} に {:?}",
                case.text,
                case.query
            );
        }
    }

    #[test]
    fn contains_agrees_with_the_shared_cases() {
        for case in cases().contains {
            assert_eq!(
                contains(&case.text, &parse_terms(&case.query)),
                case.hit,
                "{:?} に {:?}",
                case.text,
                case.query
            );
        }
    }
}
