//! マニュアル (`/help`) の配信 (→ docs/help.md)。
//!
//! `docs/manual/{ロケール}/*.md` をバイナリに埋め込み、匿名で読める一覧・個別ページの
//! APIとして返す。Markdown → HTML のレンダリングはフロントエンド側で行う (バイナリサイズと
//! サニタイズの責任を増やさないため、サーバーはMarkdown文字列を返すだけ)。

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::{Path, Query};
use axum::http::header;
use axum::response::IntoResponse;
use rust_embed::{Embed, EmbeddedFile};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::error::AppError;
use crate::state::AppState;

/// `frontend/build` (`static_files.rs`) と同じ仕組みで `docs/manual` を埋め込む。
/// ディレクトリ自体が無いとコンパイルできない点も同様。
#[derive(Embed)]
#[folder = "docs/manual"]
struct ManualAssets;

/// マニュアルの原典。要求されたロケールに該当ファイルが無ければこれを返す
/// (→ docs/help.md)。
const SOURCE_LOCALE: &str = "ja";

/// マニュアルの表示言語を受けるクエリ。
#[derive(Debug, Deserialize, IntoParams)]
struct LocaleQuery {
    /// 表示言語。`docs/manual/` 配下のディレクトリ名 (`ja` が原典)。
    /// 知らない値・未指定は原典として扱い、422にはしない (→ docs/help.md)。
    #[serde(default)]
    locale: Option<String>,
}

/// 埋め込まれているロケール (`docs/manual/` 直下のディレクトリ名)。
fn known_locales() -> BTreeSet<String> {
    ManualAssets::iter()
        .filter_map(|path| path.split_once('/').map(|(locale, _)| locale.to_string()))
        .collect()
}

/// クエリの値をロケールとして解決する。埋め込まれていないものは原典として扱う。
///
/// **受け取った値をそのままパスに使わず、ここで既知の集合に畳む**。
/// `ManualAssets::get` はリリースビルドでは完全一致だが、debug ビルドでは
/// ディスクから読むため大文字小文字の扱いがファイルシステム任せになる。
/// ここで閉じておけば、その差も外部入力がパスに混ざる余地も無くなる。
fn resolve_locale(value: Option<&str>) -> String {
    match value {
        Some(locale) if known_locales().contains(locale) => locale.to_string(),
        _ => SOURCE_LOCALE.to_string(),
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct HelpPageSummary {
    slug: String,
    title: String,
    /// このページを実際に返した言語。訳が無ければ原典になる。
    locale: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct HelpPageResponse {
    slug: String,
    title: String,
    /// Markdown原文。サニタイズはしていない (ソースは自分たちで書くものだけのため)。
    body: String,
    /// 本文を実際に返した言語。訳が無ければ原典になる。
    /// 画面はこれを `lang` 属性に使う (要求した言語をそのまま使うと、フォールバック
    /// したページで嘘の `lang` が付く → docs/help.md)。
    locale: String,
}

/// ファイル名からslugを作る。`01-intro.md` → `intro`。
/// 連番は目次の並び順を保つためだけのファイル名接頭辞で、URLには出さない。
fn slug_from_filename(filename: &str) -> Option<String> {
    let stem = filename.strip_suffix(".md")?;
    let slug = stem.split_once('-').map(|(_, rest)| rest).unwrap_or(stem);
    Some(slug.to_string())
}

/// Markdown本文の先頭の `# ` 行からタイトルを取り出す。無ければ `fallback` を使う
/// (想定外のフォーマットでも一覧表示自体は壊れないようにするため)。
fn title_from_body(body: &str, fallback: &str) -> String {
    body.lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(str::trim)
        .unwrap_or(fallback)
        .to_string()
}

/// 目次に載せるファイル名を、ファイル名順 (= 連番順 = 目次の並び順) に返す。
///
/// **並びも収録範囲も原典 (`docs/manual/ja/`) が決める**。訳が追いついていない言語でも
/// 目次の構成が変わらないようにするため、訳だけにあるファイルは目次に出さない。
fn sorted_filenames() -> Vec<String> {
    let prefix = format!("{SOURCE_LOCALE}/");
    // 画像 (`images/`) はページではないので除く。
    let mut filenames: Vec<String> = ManualAssets::iter()
        .filter_map(|path| path.strip_prefix(&prefix).map(str::to_string))
        .filter(|name| !name.contains('/') && name.ends_with(".md"))
        .collect();
    filenames.sort();
    filenames
}

/// 要求されたロケールのページを読む。訳が無ければ原典に落とす。
/// 返すロケールは**実際に読めた方**で、要求された方ではない。
fn get_file(locale: &str, filename: &str) -> Option<(String, EmbeddedFile)> {
    if let Some(file) = ManualAssets::get(&format!("{locale}/{filename}")) {
        return Some((locale.to_string(), file));
    }
    ManualAssets::get(&format!("{SOURCE_LOCALE}/{filename}"))
        .map(|file| (SOURCE_LOCALE.to_string(), file))
}

/// 埋め込みファイルの中身をUTF-8として読む。
fn body_of(file: &EmbeddedFile) -> Result<&str, AppError> {
    std::str::from_utf8(&file.data)
        .map_err(|_| AppError::DataIntegrity("manual page is not valid utf-8"))
}

#[utoipa::path(
    get,
    path = "/help/pages",
    params(LocaleQuery),
    responses((status = OK, body = Vec<HelpPageSummary>, description = "マニュアルページの一覧"))
)]
async fn list_pages(Query(query): Query<LocaleQuery>) -> Json<Vec<HelpPageSummary>> {
    let requested = resolve_locale(query.locale.as_deref());
    let pages = sorted_filenames()
        .into_iter()
        .filter_map(|filename| {
            let (locale, file) = get_file(&requested, &filename)?;
            let body = body_of(&file).ok()?;
            let slug = slug_from_filename(&filename)?;
            let title = title_from_body(body, &slug);
            Some(HelpPageSummary {
                slug,
                title,
                locale,
            })
        })
        .collect();
    Json(pages)
}

#[utoipa::path(
    get,
    path = "/help/pages/{slug}",
    params(("slug" = String, Path), LocaleQuery),
    responses(
        (status = OK, body = HelpPageResponse, description = "マニュアルページ"),
        (status = 404, body = crate::error::ErrorResponse, description = "該当slugのページが無い"),
    )
)]
async fn get_page(
    Path(slug): Path<String>,
    Query(query): Query<LocaleQuery>,
) -> Result<Json<HelpPageResponse>, AppError> {
    let requested = resolve_locale(query.locale.as_deref());
    let filename = sorted_filenames()
        .into_iter()
        .find(|filename| slug_from_filename(filename).as_deref() == Some(slug.as_str()))
        .ok_or(AppError::NotFound)?;
    let (locale, file) = get_file(&requested, &filename).ok_or(AppError::NotFound)?;
    let body = body_of(&file)?.to_string();
    let title = title_from_body(&body, &slug);
    Ok(Json(HelpPageResponse {
        slug,
        title,
        body,
        locale,
    }))
}

/// 画像のファイル名として受け付けるか。`images/` の直下の WebP だけを通す
/// (外部入力をパスに混ぜないため。`resolve_locale` と同じ考え)。
fn is_image_name(name: &str) -> bool {
    name.strip_suffix(".webp").is_some_and(|stem| {
        !stem.is_empty()
            && stem
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}

#[utoipa::path(
    get,
    path = "/help/images/{name}",
    params(("name" = String, Path), LocaleQuery),
    responses(
        (status = OK, content_type = "image/webp", description = "マニュアルの画像"),
        (status = 404, body = crate::error::ErrorResponse, description = "該当する画像が無い"),
    )
)]
async fn get_image(
    Path(name): Path<String>,
    Query(query): Query<LocaleQuery>,
) -> Result<impl IntoResponse, AppError> {
    if !is_image_name(&name) {
        return Err(AppError::NotFound);
    }
    let requested = resolve_locale(query.locale.as_deref());
    let (_, file) = get_file(&requested, &format!("images/{name}")).ok_or(AppError::NotFound)?;
    Ok((
        [(header::CONTENT_TYPE, "image/webp")],
        file.data.into_owned(),
    ))
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_pages))
        .routes(routes!(get_page))
        .routes(routes!(get_image))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_locales_are_the_embedded_directories() {
        let locales = known_locales();
        assert!(locales.contains(SOURCE_LOCALE));
        assert!(locales.contains("en"));
    }

    #[test]
    fn resolve_locale_falls_back_to_the_source() {
        assert_eq!(resolve_locale(Some("en")), "en");
        assert_eq!(resolve_locale(Some("ja")), "ja");
        // 埋め込まれていない値はすべて原典。大文字小文字も畳まない
        // (debug ビルドではファイルシステム任せになるため、ここで弾く)。
        assert_eq!(resolve_locale(Some("EN")), SOURCE_LOCALE);
        assert_eq!(resolve_locale(Some("fr")), SOURCE_LOCALE);
        assert_eq!(resolve_locale(Some("../src")), SOURCE_LOCALE);
        assert_eq!(resolve_locale(Some("")), SOURCE_LOCALE);
        assert_eq!(resolve_locale(None), SOURCE_LOCALE);
    }

    #[test]
    fn get_file_reports_the_locale_it_actually_read() {
        let (locale, _) = get_file("en", "01-intro.md").expect("英語版があるページ");
        assert_eq!(locale, "en");

        // 訳が無いページは原典に落ち、落ちたことがロケールで分かる
        // (画面はこれを `lang` に使うため、要求した方を返してはいけない)。
        let (locale, _) = get_file("fr", "01-intro.md").expect("原典は必ずある");
        assert_eq!(locale, SOURCE_LOCALE);

        // 原典にも無いファイルは見つからない。
        assert!(get_file("en", "99-missing.md").is_none());
    }

    #[test]
    fn is_image_name_accepts_only_plain_webp_names() {
        assert!(is_image_name("intro-overview.webp"));
        assert!(!is_image_name(".webp"));
        assert!(!is_image_name("../01-intro.md"));
        assert!(!is_image_name("a/b.webp"));
        assert!(!is_image_name("Intro.webp"));
        assert!(!is_image_name("intro.png"));
    }

    #[test]
    fn sorted_filenames_lists_only_pages() {
        let names = sorted_filenames();
        assert!(
            names
                .iter()
                .all(|name| name.ends_with(".md") && !name.contains('/'))
        );
    }
}
