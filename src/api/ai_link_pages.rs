//! AI が集めるリンクのうち、一覧のページの扱い (→ docs/ai.md「並べ方」)。
//! 検索で見つかった分類のトップや一覧のページを見分け、そこに載っている個別の記事を取り出す。
//! 取り出した記事は、検索で見つかった記事と合わせて AI が話題ごとにまとめる (`ai_links::rank_stories`)。

use tokio::task::JoinSet;

use super::link_title::{self, PageLink};
use crate::error::run_blocking;
use crate::state::AppState;

/// 一覧のページを表す、パスの区切りの名前。これを含む URL は、番号が付いていても一覧として扱う
/// (`/category/12`・`/page/2` のように、番号が分類やページ送りを指すため)。
const LIST_SEGMENTS: &[&str] = &[
    "archive",
    "archives",
    "categories",
    "category",
    "genre",
    "list",
    "page",
    "ranking",
    "search",
    "section",
    "tag",
    "tags",
    "theme",
    "topics",
];

/// ページ送りの値の名前。この名前の値に付いた番号は、記事の番号でない。
const PAGING_KEYS: &[&str] = &["p", "page", "offset", "start"];

/// 記事のリンクとみなす、リンクの文字の最短の長さ (文字)。見出しは文になり、「政治」「次へ」のような
/// 案内の操作の名前より長い。この長さで、メニューや送りの操作を記事と取り違えないようにする。
const MIN_ARTICLE_TEXT_CHARS: usize = 8;

/// URL が個別の記事らしいか。記事の URL は、番号か日付 (`/articles/12345`・`/2026/10/18/k100…html`)、
/// または語をつないだ見出し (`/my-first-post`) を持つ。分類や一覧のページは、短い名前だけ (`/news/`・`/politics`) になる。
/// 当てずっぽうの見分けなので、外れても困らない使い方 (並べる順・開いてみる候補) にだけ使う。
pub(super) fn looks_like_article(url: &str) -> bool {
    let Ok(url) = url::Url::parse(url) else {
        return false;
    };
    let segments: Vec<String> = url
        .path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .map(str::to_ascii_lowercase)
                .collect()
        })
        .unwrap_or_default();
    // `list.html` のように拡張子が付いていても、名前で見る。
    if segments.iter().any(|segment| {
        let stem = segment
            .split_once('.')
            .map_or(segment.as_str(), |(stem, _)| stem);
        LIST_SEGMENTS.contains(&stem)
    }) {
        return false;
    }
    let numbered_query = url.query_pairs().any(|(key, value)| {
        !PAGING_KEYS.contains(&key.to_ascii_lowercase().as_str())
            && value.chars().any(|c| c.is_ascii_digit())
    });
    let Some(last) = segments.last() else {
        return numbered_query;
    };
    let stem = last
        .rsplit_once('.')
        .map_or(last.as_str(), |(stem, _)| stem);
    if matches!(stem, "index" | "default" | "top") {
        return numbered_query;
    }
    let words = stem
        .split(['-', '_'])
        .filter(|word| !word.is_empty())
        .count();
    stem.chars().any(|c| c.is_ascii_digit()) || words >= 3 || numbered_query
}

/// `www.` を外したホスト名。
fn host(url: &str) -> Option<String> {
    let host = url::Url::parse(url).ok()?.host_str()?.to_ascii_lowercase();
    Some(
        host.strip_prefix("www.")
            .map_or(host.clone(), str::to_string),
    )
}

/// 同じサイトか。ニュースのサイトは記事を別のサブドメインに置くことがある (`www.example.jp` の一覧から
/// `news.example.jp` の記事へ) ので、片方がもう片方のサブドメインでもよい。
fn same_site(a: &str, b: &str) -> bool {
    a == b || a.ends_with(&format!(".{b}")) || b.ends_with(&format!(".{a}"))
}

/// 一覧のページのリンクのうち、同じサイトの個別の記事らしいもの。出てくる順のまま。
pub(super) fn article_links(list_url: &str, links: Vec<PageLink>) -> Vec<PageLink> {
    let Some(list_host) = host(list_url) else {
        return Vec::new();
    };
    links
        .into_iter()
        .filter(|link| {
            link.url != list_url
                && link.text.chars().count() >= MIN_ARTICLE_TEXT_CHARS
                && host(&link.url).is_some_and(|host| same_site(&host, &list_host))
                && looks_like_article(&link.url)
        })
        .collect()
}

/// 一覧のページを開き、載っている記事を返す。並びは `lists` の順、その中は載っている順。
/// 開けなかったページは飛ばす。外へ取りに行く数は、カードの取り直しと同じ枠で絞る。
pub(super) async fn open_lists(state: &AppState, lists: &[String]) -> Vec<PageLink> {
    let mut tasks = JoinSet::new();
    for (order, url) in lists.iter().enumerate() {
        let state = state.clone();
        let url = url.clone();
        tasks.spawn(async move {
            let _permit = state.link_previews.permit().await;
            let fetched = {
                let url = url.clone();
                run_blocking(move || link_title::fetch_links_blocking(&url)).await
            };
            let links = fetched.ok().flatten().unwrap_or_default();
            (order, article_links(&url, links))
        });
    }
    let mut opened = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        if let Ok(found) = joined {
            opened.push(found);
        }
    }
    opened.sort_by_key(|(order, _)| *order);
    opened.into_iter().flat_map(|(_, links)| links).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(url: &str, text: &str) -> PageLink {
        PageLink {
            url: url.to_string(),
            text: text.to_string(),
        }
    }

    #[test]
    fn numbered_dated_and_slug_urls_look_like_articles() {
        for url in [
            "https://www.city.mobara.chiba.jp/0000012345.html",
            "https://www3.nhk.or.jp/news/html/20261018/k10014000000000.html",
            "https://www.asahi.com/articles/AST1A2B3C.html",
            "https://natalie.mu/music/news/612345",
            "https://blog.example/2026/10/18/my-first-balcony-garden",
            "https://example.jp/news/detail?id=123",
        ] {
            assert!(looks_like_article(url), "{url}");
        }
    }

    #[test]
    fn tops_sections_and_paging_do_not_look_like_articles() {
        for url in [
            "https://www.chibanippo.co.jp/",
            "https://www.chibanippo.co.jp/news/",
            "https://www.asahi.com/politics/",
            "https://news.yahoo.co.jp/categories/domestic",
            "https://example.jp/category/12",
            "https://example.jp/news/page/2",
            "https://example.jp/news/index.html",
            "https://example.jp/news/?page=3",
            "https://example.jp/about-us",
            "https://www.asahi.com/rensai/list.html?id=536",
            "https://www.nikkei.com/theme/?dw=26073100",
        ] {
            assert!(!looks_like_article(url), "{url}");
        }
    }

    #[test]
    fn article_links_keep_titled_articles_of_the_same_site_in_order() {
        let links = vec![
            link("https://www.example.jp/politics/", "政治のニュース一覧へ"),
            link(
                "https://www.example.jp/articles/2",
                "知事が来年度の予算案を発表",
            ),
            link("https://www.example.jp/articles/3", "次へ"),
            link(
                "https://other.example/articles/4",
                "ほかのサイトの長い見出しの記事",
            ),
            link(
                "https://news.example.jp/articles/1",
                "市議会で新しい条例が成立",
            ),
            link("https://www.example.jp/news/", "このページそのもの"),
        ];
        let found: Vec<String> = article_links("https://www.example.jp/news/", links)
            .into_iter()
            .map(|link| link.url)
            .collect();
        assert_eq!(
            found,
            [
                "https://www.example.jp/articles/2",
                "https://news.example.jp/articles/1",
            ]
        );
    }
}
