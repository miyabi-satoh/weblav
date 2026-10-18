//! ページを最後に更新した日時を、標準の書き方から読む (→ docs/ui.md「リンクのカード」)。
//! 読むのは、ページが日時を伝えるために決まっている書き方 (schema.org の JSON-LD と microdata・OGP・Dublin Core・HTTP の `Last-Modified`) だけ。
//! 本文の文字から日付らしいものを拾うことはしない。関連の記事やコメントの日付を取り違えるため。

use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

use super::link_title::tags;

/// 更新した日時を伝える `<meta>` の名前。前ほど優先する。
const MODIFIED_META: &[&str] = &[
    "article:modified_time",
    "og:updated_time",
    "dcterms.modified",
    "dc.date.modified",
];

/// 公開した日時を伝える `<meta>` の名前。更新した日時が無いときに使う。前ほど優先する。
const PUBLISHED_META: &[&str] = &[
    "article:published_time",
    "dcterms.issued",
    "dcterms.created",
    "dc.date.issued",
    "dcterms.date",
    "dc.date",
    "date",
];

/// ページを最後に更新した日時。更新した日時が無ければ公開した日時。`lower` は `html` を小文字にしたもの。
/// 形は、時差の分かるものは UTC の RFC 3339、日付だけのものは `YYYY-MM-DD`、時差の無いものは時差を付けない ISO 8601 (画面の地域の時刻として読まれる)。
pub(super) fn updated_at(html: &str, lower: &str) -> Option<String> {
    let json_ld = json_ld_nodes(html, lower);
    let microdata = microdata(html, lower);
    let from_json_ld = |key: &str| {
        // 記事の節を先に見る。同じ JSON-LD のサイトやページの節は、サイト全体の日時を持つことがあるため。
        let (articles, others): (Vec<&Value>, Vec<&Value>) =
            json_ld.iter().partition(|node| is_article(node));
        articles
            .into_iter()
            .chain(others)
            .find_map(|node| node.get(key)?.as_str().and_then(normalize))
    };
    let from_microdata = |prop: &str| {
        microdata
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case(prop))
            .find_map(|(_, value)| normalize(value))
    };
    let from_meta = |names: &[&str]| names.iter().find_map(|name| meta(html, lower, name));
    from_json_ld("dateModified")
        .or_else(|| from_meta(&MODIFIED_META[..2]))
        .or_else(|| from_microdata("dateModified"))
        .or_else(|| from_meta(&MODIFIED_META[2..]))
        .or_else(|| from_json_ld("datePublished"))
        .or_else(|| from_json_ld("dateCreated"))
        .or_else(|| from_meta(&PUBLISHED_META[..1]))
        .or_else(|| from_microdata("datePublished"))
        .or_else(|| from_meta(&PUBLISHED_META[1..]))
}

/// HTTP の `Last-Modified`。ページに日時の書き込みが無いときに使う。
/// 応答の `Date` と同じ時刻なら使わない。その場で組み立てるページは、作った時刻を最終更新として送るため。
pub(super) fn from_last_modified(last_modified: &str, date: Option<&str>) -> Option<String> {
    let modified = http_date(last_modified)?;
    if date
        .and_then(http_date)
        .is_some_and(|date| (date - modified).whole_seconds().abs() < GENERATED_SECONDS)
    {
        return None;
    }
    modified.format(&Rfc3339).ok()
}

/// `Last-Modified` が `Date` からこの秒数より近ければ、ページを作った時刻とみなす。
/// 組み立てと送り出しの間の遅れを吸収できる長さにし、本当にさっき直したページの日時を捨てすぎないようにする。
const GENERATED_SECONDS: i64 = 60;

/// RFC 9110 の HTTP-date (IMF-fixdate)。
fn http_date(raw: &str) -> Option<OffsetDateTime> {
    let format = time::macros::format_description!(
        "[weekday repr:short], [day] [month repr:short] [year] [hour]:[minute]:[second] GMT"
    );
    Some(
        PrimitiveDateTime::parse(raw.trim(), &format)
            .ok()?
            .assume_utc(),
    )
}

fn meta(html: &str, lower: &str, name: &str) -> Option<String> {
    tags(html, lower, "meta").find_map(|attrs| {
        let matches_name = attrs.iter().any(|(attr, value)| {
            (attr == "property" || attr == "name") && value.trim().eq_ignore_ascii_case(name)
        });
        if !matches_name {
            return None;
        }
        attrs
            .into_iter()
            .find(|(attr, _)| attr == "content")
            .and_then(|(_, content)| normalize(&content))
    })
}

/// microdata の `itemprop` と値。値を持つのは `<meta content>` と `<time datetime>`。
fn microdata(html: &str, lower: &str) -> Vec<(String, String)> {
    [("meta", "content"), ("time", "datetime")]
        .into_iter()
        .flat_map(|(tag, value_attr)| {
            tags(html, lower, tag).filter_map(move |attrs| {
                let prop = attrs.iter().find(|(attr, _)| attr == "itemprop")?.1.clone();
                let value = attrs.into_iter().find(|(attr, _)| attr == value_attr)?.1;
                Some((prop, value))
            })
        })
        .collect()
}

/// `<script type="application/ld+json">` の節。配列と `@graph` の中の節も、1つずつに広げる。
fn json_ld_nodes(html: &str, lower: &str) -> Vec<Value> {
    let mut nodes = Vec::new();
    let mut rest = 0;
    while let Some(found) = lower[rest..].find("<script") {
        let start = rest + found;
        let Some(open_end) = lower[start..].find('>').map(|end| start + end + 1) else {
            break;
        };
        let Some(close) = lower[open_end..].find("</script").map(|end| open_end + end) else {
            break;
        };
        rest = close;
        if !lower[start..open_end].contains("application/ld+json") {
            continue;
        }
        // CMS が `<!-- -->` や CDATA で囲むことがある。
        let body = html[open_end..close]
            .trim()
            .trim_start_matches("<!--")
            .trim_end_matches("-->")
            .trim()
            .trim_start_matches("<![CDATA[")
            .trim_end_matches("]]>");
        if let Ok(value) = serde_json::from_str::<Value>(body) {
            flatten(value, &mut nodes);
        }
    }
    nodes
}

fn flatten(value: Value, nodes: &mut Vec<Value>) {
    match value {
        Value::Array(items) => items.into_iter().for_each(|item| flatten(item, nodes)),
        Value::Object(mut object) => {
            if let Some(graph) = object.remove("@graph") {
                flatten(graph, nodes);
            }
            nodes.push(Value::Object(object));
        }
        _ => {}
    }
}

/// schema.org の記事の型か (`Article`・`NewsArticle`・`BlogPosting`・`Report` など)。
fn is_article(node: &Value) -> bool {
    let is_article_type =
        |name: &str| name.ends_with("Article") || name.ends_with("Posting") || name == "Report";
    match node.get("@type") {
        Some(Value::String(name)) => is_article_type(name),
        Some(Value::Array(names)) => names.iter().filter_map(Value::as_str).any(is_article_type),
        _ => false,
    }
}

/// ISO 8601 の日時 (schema.org・OGP・Dublin Core が使う形) を読み、揃えた形で返す。読めなければ `None`。
/// 区切りの `T` の代わりの空白と、コロンの無い時差 (`+0900`) は、よく見かけるので受け付ける。
fn normalize(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let date = parse_date(raw.get(..10)?)?;
    let rest = &raw[10..];
    if rest.is_empty() {
        return Some(raw.to_string());
    }
    let rest = rest.strip_prefix(['T', 't', ' '])?;
    let time_end = rest
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, ':' | '.' | ',')))
        .unwrap_or(rest.len());
    let at = PrimitiveDateTime::new(date, parse_time(&rest[..time_end])?);
    let zone = rest[time_end..].trim();
    if zone.is_empty() {
        let format =
            time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]");
        return at.format(&format).ok();
    }
    at.assume_offset(parse_offset(zone)?)
        .to_offset(UtcOffset::UTC)
        .format(&Rfc3339)
        .ok()
}

fn parse_date(text: &str) -> Option<Date> {
    let bytes = text.as_bytes();
    if bytes.get(4) != Some(&b'-') || bytes.get(7) != Some(&b'-') {
        return None;
    }
    let year = text[..4].parse().ok()?;
    let month = Month::try_from(text[5..7].parse::<u8>().ok()?).ok()?;
    Date::from_calendar_date(year, month, text[8..10].parse().ok()?).ok()
}

/// `hh:mm`・`hh:mm:ss`・`hh:mm:ss.fff`。秒の端数は捨てる。
fn parse_time(text: &str) -> Option<Time> {
    let text = text.split(['.', ',']).next()?;
    let mut parts = text.split(':');
    let hour = parts.next()?.parse().ok()?;
    let minute = parts.next()?.parse().ok()?;
    let second = parts.next().map_or(Some(0), |second| second.parse().ok())?;
    if parts.next().is_some() {
        return None;
    }
    Time::from_hms(hour, minute, second).ok()
}

/// `Z`・`+09:00`・`+0900`・`+09`。
fn parse_offset(text: &str) -> Option<UtcOffset> {
    if text.eq_ignore_ascii_case("z") {
        return Some(UtcOffset::UTC);
    }
    let sign: i8 = match text.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let digits: String = text[1..].chars().filter(|&c| c != ':').collect();
    if !matches!(digits.len(), 2 | 4) || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let hours: i8 = digits[..2].parse().ok()?;
    let minutes: i8 = digits
        .get(2..)
        .map_or(Some(0), |minutes| minutes.parse().ok())?;
    UtcOffset::from_hms(sign * hours, sign * minutes, 0).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(html: &str) -> Option<String> {
        updated_at(html, &html.to_ascii_lowercase())
    }

    #[test]
    fn normalize_reads_the_iso_8601_forms_pages_use() {
        assert_eq!(
            normalize("2026-09-28T17:00:00+09:00").as_deref(),
            Some("2026-09-28T08:00:00Z")
        );
        assert_eq!(
            normalize("2026-09-28T17:00+09:00").as_deref(),
            Some("2026-09-28T08:00:00Z")
        );
        assert_eq!(
            normalize("2026-09-28 17:00:00 +0900").as_deref(),
            Some("2026-09-28T08:00:00Z")
        );
        assert_eq!(
            normalize("2026-09-28T08:00:00.123Z").as_deref(),
            Some("2026-09-28T08:00:00Z")
        );
        assert_eq!(
            normalize("2026-09-28T17:00:00").as_deref(),
            Some("2026-09-28T17:00:00")
        );
        assert_eq!(normalize(" 2026-09-28 ").as_deref(), Some("2026-09-28"));
        for broken in [
            "2026/09/28",
            "2026-13-01",
            "2026-09-28T25:00",
            "2026-09-28Tnoon",
            "yesterday",
            "",
        ] {
            assert_eq!(normalize(broken), None, "{broken}");
        }
    }

    #[test]
    fn modified_dates_win_over_published_ones_whatever_the_format() {
        let html = r#"<meta property="article:published_time" content="2026-09-01T00:00:00Z">
            <meta property="article:modified_time" content="2026-09-02T00:00:00Z">"#;
        assert_eq!(read(html).as_deref(), Some("2026-09-02T00:00:00Z"));
        let html = r#"<script type="application/ld+json">{"@type":"NewsArticle","datePublished":"2026-09-01"}</script>
            <meta name="dcterms.modified" content="2026-09-03">"#;
        assert_eq!(read(html).as_deref(), Some("2026-09-03"));
    }

    #[test]
    fn json_ld_prefers_the_article_in_a_graph_over_the_site() {
        let html = r#"<script type="application/ld+json">
            {"@context":"https://schema.org","@graph":[
                {"@type":"WebSite","dateModified":"2026-10-18T00:00:00Z"},
                {"@type":["NewsArticle"],"datePublished":"2026-09-28T17:00:00+09:00","dateModified":"2026-09-29T09:00:00+09:00"}
            ]}</script>"#;
        assert_eq!(read(html).as_deref(), Some("2026-09-29T00:00:00Z"));
    }

    #[test]
    fn microdata_dublin_core_and_the_plain_date_meta_are_read() {
        assert_eq!(
            read(r#"<time itemprop="datePublished" datetime="2026-09-28">9月28日</time>"#)
                .as_deref(),
            Some("2026-09-28")
        );
        assert_eq!(
            read(r#"<meta name="DC.date.issued" content="2026-09-27">"#).as_deref(),
            Some("2026-09-27")
        );
        assert_eq!(
            read(r#"<meta name="date" content="2026-09-28T17:00+09:00" />"#).as_deref(),
            Some("2026-09-28T08:00:00Z")
        );
    }

    #[test]
    fn unreadable_values_fall_through_to_the_next_source() {
        let html = r#"<script type="application/ld+json">{"@type":"Article","dateModified":"先週"}</script>
            <meta property="article:published_time" content="2026-09-01">"#;
        assert_eq!(read(html).as_deref(), Some("2026-09-01"));
        assert_eq!(read("<p>2026-09-01 に公開</p>"), None);
    }

    #[test]
    fn last_modified_is_used_unless_it_is_the_time_the_page_was_generated() {
        let date = Some("Sun, 18 Oct 2026 04:15:21 GMT");
        assert_eq!(
            from_last_modified("Sat, 17 Oct 2026 15:31:18 GMT", date).as_deref(),
            Some("2026-10-17T15:31:18Z")
        );
        assert_eq!(
            from_last_modified("Sun, 18 Oct 2026 04:15:20 GMT", date),
            None
        );
        assert_eq!(
            from_last_modified("Sat, 17 Oct 2026 15:31:18 GMT", None).as_deref(),
            Some("2026-10-17T15:31:18Z")
        );
        assert_eq!(from_last_modified("17 Oct 2026", date), None);
    }
}
