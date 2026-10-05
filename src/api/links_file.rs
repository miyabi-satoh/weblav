//! リンクの一覧のファイル (`.links.toml`) を読んで、中のリンクを返す (→ docs/ui.md「リンクの一覧のファイル」)。

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::archive_items;
use super::contents::{self, ContentType, GroupAncestor, Lineage, ServedFile};
use super::link_preview::{self, LinkPreview};
use crate::auth::Viewer;
use crate::error::{AppError, run_blocking};
use crate::state::AppState;

/// 一覧のファイルの名前の終わり。これで終わるファイルだけを一覧として読む。
const LINKS_FILE_SUFFIX: &str = ".links.toml";

/// 読む大きさの上限。手やジョブが書くリンクの一覧は数十KBに収まるので、それより大きいものは一覧とみなさない。
const MAX_LINKS_FILE_BYTES: u64 = 1024 * 1024;

/// どの一覧のファイルか。フォルダの中は `path`、アーカイブのアイテムは `item`、`file` コンテンツは id だけで指す。
#[derive(Debug, Default, Deserialize, IntoParams, ToSchema)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub(super) struct LinksFileQuery {
    /// フォルダの登録パスからの相対パス。フォルダの中のファイルのときだけ使う。
    #[serde(default)]
    pub(super) path: String,
    /// アーカイブのアイテムの id。アーカイブの中のファイルのときだけ使う。
    pub(super) item: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct LinksFileResponse {
    /// ファイルに書かれた `title`。省かれているか読めなければファイル名。
    title: String,
    /// パンくずの最後に出す名前。`file` コンテンツは一覧に出ている登録したタイトル、ほかはファイル名。
    name: String,
    /// ルートに近い順の祖先グループ。コンテンツ自身は含まない。
    ancestors: Vec<GroupAncestor>,
    /// フォルダ・アーカイブの中のファイルなら、そのコンテンツのタイトル。`file` コンテンツなら無し。
    container_title: Option<String>,
    /// 書いた順のリンク。読めなければ無し (画面は読めなかった旨と、ファイルを新規タブで開く手段を出す)。
    links: Option<Vec<LinksFileEntry>>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct LinksFileEntry {
    url: String,
    note: Option<String>,
    /// 覚えている情報。まだ覚えていなければ無し。
    preview: Option<LinkPreview>,
}

/// 開いた一覧のファイルと、パンくずに出すもの。
struct Opened {
    file: ServedFile,
    lineage: Lineage,
    container_title: Option<String>,
    /// `file` コンテンツの登録したタイトル。
    content_title: Option<String>,
}

/// 一覧のファイルを、閲覧の権限を確かめてから指す。見られない・無い・一覧のファイルでなければ 404 か 401。
async fn open(
    state: &AppState,
    viewer: &Viewer,
    id: i64,
    query: &LinksFileQuery,
) -> Result<Opened, AppError> {
    let content_type = sqlx::query_scalar!(
        r#"SELECT type as "content_type: ContentType" FROM contents WHERE id = ?"#,
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    // どの分岐も、ファイルシステムに触る前に公開範囲を確かめる関数を通る (理由は `contents::folder_root`)。
    let opened = match (content_type, query.item) {
        (ContentType::Folder, None) => {
            let (title, root, lineage) = contents::folder_root(state, viewer, id).await?;
            let file = contents::resolve_file_under(state, &root, &query.path).await?;
            Opened {
                file,
                lineage,
                container_title: Some(title),
                content_title: None,
            }
        }
        (ContentType::Archive, Some(item)) => {
            let (meta, lineage) =
                archive_items::load_viewable_archive(&state.pool, viewer, id).await?;
            let file = archive_items::item_file(state, viewer, id, item).await?;
            Opened {
                file,
                lineage,
                container_title: Some(meta.title),
                content_title: None,
            }
        }
        (ContentType::File, None) => {
            let lineage = contents::ensure_viewable(&state.pool, viewer, id).await?;
            let file = contents::content_file(state, viewer, id, "").await?;
            let content_title = lineage.own_title();
            Opened {
                file,
                lineage,
                container_title: None,
                content_title: Some(content_title),
            }
        }
        _ => return Err(AppError::NotFound),
    };
    if !is_links_file_name(&opened.file.file_name) {
        return Err(AppError::NotFound);
    }
    Ok(opened)
}

fn is_links_file_name(name: &str) -> bool {
    name.to_lowercase().ends_with(LINKS_FILE_SUFFIX)
}

/// 読み取った一覧。
#[derive(Debug, PartialEq, Eq)]
struct LinksFile {
    title: Option<String>,
    links: Vec<(String, Option<String>)>,
}

/// TOML として書かれた一覧。行ごとの誤り (`url` が無い・文字列でない) で一覧全体を読めなくしないよう、
/// 行は値のまま受けて `parse` で拾う。
#[derive(Deserialize)]
struct RawLinksFile {
    title: Option<String>,
    #[serde(default)]
    links: Vec<toml::Table>,
}

/// 一覧を読む。TOML として読めなければ `None`。`http`・`https` 以外の URL の行と、`url` の無い行は飛ばす。
fn parse(bytes: &[u8]) -> Option<LinksFile> {
    let text = std::str::from_utf8(bytes).ok()?;
    let raw: RawLinksFile = toml::from_str(text).ok()?;
    let links = raw
        .links
        .iter()
        .filter_map(|row| {
            let url = url::Url::parse(row.get("url")?.as_str()?.trim()).ok()?;
            if !matches!(url.scheme(), "http" | "https") {
                return None;
            }
            let note = row
                .get("note")
                .and_then(|note| note.as_str())
                .map(str::trim)
                .filter(|note| !note.is_empty())
                .map(str::to_string);
            Some((url.to_string(), note))
        })
        .collect();
    let title = raw
        .title
        .map(|title| title.trim().to_string())
        .filter(|title| !title.is_empty());
    Some(LinksFile { title, links })
}

/// 一覧のファイルを読む。大きすぎる・読めないものは `None`。
async fn read(file: &ServedFile) -> Result<Option<LinksFile>, AppError> {
    let path = file.path.clone();
    run_blocking(move || {
        let Ok(metadata) = std::fs::metadata(&path) else {
            return None;
        };
        if metadata.len() > MAX_LINKS_FILE_BYTES {
            return None;
        }
        parse(&std::fs::read(&path).ok()?)
    })
    .await
}

/// 一覧のファイルの中の URL。リンクのカードの取り直しで、見られる一覧のものだけを取りに行くため。
pub(super) async fn viewable_urls(
    state: &AppState,
    viewer: &Viewer,
    id: i64,
    query: &LinksFileQuery,
) -> Result<Vec<String>, AppError> {
    let opened = open(state, viewer, id, query).await?;
    Ok(read(&opened.file)
        .await?
        .map(|file| file.links.into_iter().map(|(url, _)| url).collect())
        .unwrap_or_default())
}

/// 一覧のファイルの中のリンクを、覚えているカードの情報を添えて返す。
/// カードの情報は取りに行かない。画面がこの後で取り直しを頼む (→ `link_preview`)。
#[utoipa::path(
    get,
    path = "/contents/{id}/links",
    params(("id" = i64, Path), LinksFileQuery),
    responses(
        (status = OK, body = LinksFileResponse),
        (status = 401, body = crate::error::ErrorResponse, description = "閲覧にログインが必要"),
        (status = 404, body = crate::error::ErrorResponse, description = "見えない・存在しない、または一覧のファイルでない"),
    )
)]
async fn links_file(
    viewer: Viewer,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<LinksFileQuery>,
) -> Result<Json<LinksFileResponse>, AppError> {
    let opened = open(&state, &viewer, id, &query).await?;
    let parsed = read(&opened.file).await?;
    let file_name = opened.file.file_name;
    let (title, links) = match parsed {
        Some(parsed) => {
            let urls: Vec<String> = parsed.links.iter().map(|(url, _)| url.clone()).collect();
            let previews = link_preview::cached(&state.pool, &urls).await?;
            let links = parsed
                .links
                .into_iter()
                .map(|(url, note)| LinksFileEntry {
                    preview: previews.get(&url).cloned(),
                    url,
                    note,
                })
                .collect();
            (parsed.title, Some(links))
        }
        None => (None, None),
    };
    Ok(Json(LinksFileResponse {
        title: title.unwrap_or_else(|| file_name.clone()),
        name: opened.content_title.unwrap_or(file_name),
        ancestors: opened.lineage.breadcrumbs(),
        container_title: opened.container_title,
        links,
    }))
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(links_file))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keeps_the_written_order_and_skips_rows_it_cannot_open() {
        let parsed = parse(
            r#"
title = " 今朝の記事 "

[[links]]
url = "https://example.com/a"
note = "最初"

[[links]]
url = "javascript:alert(1)"

[[links]]
note = "url が無い"

[[links]]
url = 42

[[links]]
url = "http://example.com/b"
note = " "
"#
            .as_bytes(),
        )
        .expect("読めるはず");

        assert_eq!(
            parsed,
            LinksFile {
                title: Some("今朝の記事".to_string()),
                links: vec![
                    (
                        "https://example.com/a".to_string(),
                        Some("最初".to_string())
                    ),
                    ("http://example.com/b".to_string(), None),
                ],
            }
        );
    }

    #[test]
    fn parse_leaves_the_title_empty_when_omitted() {
        let parsed = parse(b"[[links]]\nurl = \"https://example.com/\"\n").expect("読めるはず");
        assert_eq!(parsed.title, None, "画面はファイル名を出す");
        assert_eq!(parsed.links.len(), 1);
    }

    #[test]
    fn parse_rejects_files_that_are_not_toml_or_utf8() {
        assert_eq!(parse(b"links = [\n"), None, "TOML として壊れている");
        assert_eq!(parse(b"links = \"text\"\n"), None, "links が表の並びでない");
        assert_eq!(parse(&[0xff, 0xfe, 0x00]), None, "UTF-8 でない");
    }

    #[test]
    fn links_file_names_are_matched_without_case() {
        assert!(is_links_file_name("今朝.links.toml"));
        assert!(is_links_file_name("A.LINKS.TOML"));
        assert!(!is_links_file_name("links.toml"));
        assert!(!is_links_file_name("a.toml"));
    }
}
