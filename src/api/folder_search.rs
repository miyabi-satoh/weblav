//! 検索の、フォルダーの中とリンクの一覧のファイルの中 (→ docs/search.md「フォルダーの中」「リンクの一覧のファイルの中」)。
//!
//! どちらも索引を持たず、検索のたびにディスクを読む。見える範囲は、呼び出し側が渡すフォルダー・アーカイブ・
//! `file` コンテンツ (閲覧者が一覧で見られるもの) で決まる。

use std::path::PathBuf;

use serde::Serialize;
use utoipa::ToSchema;

use super::archive_items;
use super::contents::{self, FolderEntry};
use super::link_preview::{self, LinkPreview};
use super::links_file;
use super::roots::OwnDirs;
use super::search::{RESULT_LIMIT, Terms, normalize};
use super::sort::title_cmp;
use crate::auth::Viewer;
use crate::error::{AppError, run_blocking};
use crate::state::AppState;

/// ADR: 1つのフォルダーで、検索のたびに辿るエントリ数の上限。
/// 索引を持たずに打つたび辿るので、巨大なツリーを指したフォルダーで検索ごと遅くならないよう抑える。
/// 登録前の確認・アーカイブの索引の上限 (`fs::ITEM_LIMIT`) と同じ桁にしている。
const FOLDER_WALK_LIMIT: usize = 20_000;

/// 検索の対象にするフォルダー。閲覧者が一覧で見られることは、呼び出し側が確かめる。
pub(super) struct SearchableFolder {
    pub(super) id: i64,
    pub(super) title: String,
    pub(super) path: String,
    /// 辿り始める階層 (登録パスからの相対パス)。フォルダー全体なら空。`contents::search_scope` で確かめ済み。
    pub(super) sub_path: String,
}

/// リンクの一覧のファイルの `file` コンテンツ。閲覧者が一覧で見られることは、呼び出し側が確かめる。
pub(super) struct LinksFileContent {
    pub(super) id: i64,
    pub(super) title: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct SearchFileHit {
    pub(super) content_id: i64,
    /// 行の2段目に、どこにあるかとして出す。
    pub(super) folder_title: String,
    /// フォルダーの登録パスからの相対パス (`/` 区切り)。エントリ自身の名前まで含む。
    pub(super) path: String,
    pub(super) entry: FolderEntry,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct SearchLinkHit {
    /// 一覧のファイルを持つコンテンツ (フォルダー・アーカイブ・`file`)。
    pub(super) content_id: i64,
    /// フォルダーの中の一覧なら、その相対パス。
    pub(super) path: Option<String>,
    /// アーカイブの中の一覧なら、そのアイテムの id。
    pub(super) item: Option<i64>,
    /// 一覧のファイルの題。`title` が無ければ、`file` コンテンツは登録したタイトル、ほかはファイル名。
    pub(super) file_title: String,
    /// フォルダー・アーカイブの中の一覧なら、そのコンテンツのタイトル。
    pub(super) container_title: Option<String>,
    pub(super) url: String,
    pub(super) note: Option<String>,
    pub(super) preview: Option<LinkPreview>,
    /// 題では当たらず、`note` で当たったか。何で当たったか分かるよう、画面が `note` を添える。
    pub(super) matched_in_note: bool,
}

pub(super) struct FolderSearch {
    pub(super) files: Vec<SearchFileHit>,
    pub(super) files_truncated: bool,
    /// 辿るエントリ数の上限で、辿りきれなかったフォルダーがあるか。その先のファイルと一覧のファイルは探していない。
    pub(super) incomplete: bool,
    pub(super) links: Vec<SearchLinkHit>,
    pub(super) links_truncated: bool,
}

/// 読む一覧のファイルと、それがどこにあるか。
struct LinksSource {
    content_id: i64,
    path: Option<String>,
    item: Option<i64>,
    container_title: Option<String>,
    /// `file` コンテンツの登録したタイトル。
    content_title: Option<String>,
    file: PathBuf,
    file_name: String,
}

/// 名前で当たったフォルダーの中のエントリ。行の形にするのは打ち切った後の分だけ。
struct FileCandidate {
    folder: usize,
    rel_path: String,
    name: String,
    absolute: PathBuf,
}

/// フォルダーの中のファイル・ディレクトリを名前で、リンクの一覧のファイルの中のリンクを題と `note` で探す。
pub(super) async fn search(
    state: &AppState,
    viewer: &Viewer,
    folders: Vec<SearchableFolder>,
    archives: Vec<(i64, String)>,
    links_files: Vec<LinksFileContent>,
    terms: &Terms,
) -> Result<FolderSearch, AppError> {
    let roots = super::roots::load_roots(&state.pool).await?;
    let own_dirs = state.own_dirs.clone();
    let walk_terms = terms.clone();
    let folder_ids: Vec<i64> = folders.iter().map(|folder| folder.id).collect();
    let folder_titles: Vec<String> = folders.iter().map(|folder| folder.title.clone()).collect();

    // 辿るのはディスクの I/O で、フォルダーの大きさに比例して重くなる。
    let (mut candidates, incomplete, mut sources) = run_blocking(move || {
        let own_dirs = OwnDirs::resolve(&own_dirs);
        let mut candidates = Vec::new();
        let mut truncated = false;
        let mut sources = Vec::new();
        for (index, folder) in folders.into_iter().enumerate() {
            // 配信と同じく、公開できるフォルダーの外は辿らない (→ contents::resolve_path)。
            let Ok(root) = std::fs::canonicalize(&folder.path) else {
                continue;
            };
            if !super::roots::is_within_roots(&roots, &root, &own_dirs) {
                continue;
            }
            let (entries, cut) =
                super::fs::walk_entries(&root.join(&folder.sub_path), FOLDER_WALK_LIMIT, &own_dirs);
            truncated |= cut;
            for mut entry in entries {
                // 結果のパスは、階層の中からでなく、フォルダーの登録パスからの相対にする。
                if !folder.sub_path.is_empty() {
                    entry.rel_path = format!("{}/{}", folder.sub_path, entry.rel_path);
                }
                let name = entry
                    .rel_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&entry.rel_path)
                    .to_string();
                let absolute = root.join(&entry.rel_path);
                if !entry.is_dir && links_file::is_links_file_name(&name) {
                    sources.push(LinksSource {
                        content_id: folder.id,
                        path: Some(entry.rel_path.clone()),
                        item: None,
                        container_title: Some(folder.title.clone()),
                        content_title: None,
                        file: absolute.clone(),
                        file_name: name.clone(),
                    });
                }
                if walk_terms.matches(&[&normalize(&name)]) {
                    candidates.push(FileCandidate {
                        folder: index,
                        rel_path: entry.rel_path,
                        name,
                        absolute,
                    });
                }
            }
        }
        (candidates, truncated, sources)
    })
    .await?;

    candidates.sort_by(|a, b| {
        title_cmp(&a.name, &b.name)
            .then_with(|| title_cmp(&folder_titles[a.folder], &folder_titles[b.folder]))
            .then_with(|| a.rel_path.cmp(&b.rel_path))
    });
    let files_truncated = candidates.len() > RESULT_LIMIT;
    candidates.truncate(RESULT_LIMIT);
    let files = run_blocking(move || {
        candidates
            .into_iter()
            .filter_map(|candidate| {
                let entry = contents::folder_entry(candidate.name, &candidate.absolute)?;
                Some(SearchFileHit {
                    content_id: folder_ids[candidate.folder],
                    folder_title: folder_titles[candidate.folder].clone(),
                    path: candidate.rel_path,
                    entry,
                })
            })
            .collect::<Vec<_>>()
    })
    .await?;

    sources.extend(archive_sources(state, viewer, archives).await?);
    sources.extend(file_sources(state, viewer, links_files).await?);
    let (links, links_truncated) = search_links(state, sources, terms).await?;

    Ok(FolderSearch {
        files,
        files_truncated,
        incomplete,
        links,
        links_truncated,
    })
}

/// アーカイブの公開アイテムのうち、リンクの一覧のファイル。配信と同じ確かめ (`item_file`) を通ったものだけ。
async fn archive_sources(
    state: &AppState,
    viewer: &Viewer,
    archives: Vec<(i64, String)>,
) -> Result<Vec<LinksSource>, AppError> {
    let mut sources = Vec::new();
    for (archive_id, archive_title) in archives {
        let items = sqlx::query!(
            r#"SELECT id as "id!", rel_path FROM archive_items WHERE archive_id = ? AND published = 1"#,
            archive_id
        )
        .fetch_all(&state.pool)
        .await?;
        for item in items {
            if !links_file::is_links_file_name(&item.rel_path) {
                continue;
            }
            // 消えた・隠しになったものは、一覧と同じく出さない。
            let Ok(file) = archive_items::item_file(state, viewer, archive_id, item.id).await
            else {
                continue;
            };
            sources.push(LinksSource {
                content_id: archive_id,
                path: None,
                item: Some(item.id),
                container_title: Some(archive_title.clone()),
                content_title: None,
                file: file.path,
                file_name: file.file_name,
            });
        }
    }
    Ok(sources)
}

/// リンクの一覧のファイルの `file` コンテンツ。
async fn file_sources(
    state: &AppState,
    viewer: &Viewer,
    links_files: Vec<LinksFileContent>,
) -> Result<Vec<LinksSource>, AppError> {
    let mut sources = Vec::new();
    for content in links_files {
        let Ok(file) = contents::content_file(state, viewer, content.id, "").await else {
            continue;
        };
        sources.push(LinksSource {
            content_id: content.id,
            path: None,
            item: None,
            container_title: None,
            content_title: Some(content.title),
            file: file.path,
            file_name: file.file_name,
        });
    }
    Ok(sources)
}

/// 一覧のファイルを読み、中のリンクを題 (覚えているページのタイトル、無ければホスト名) と `note` で照らす。
/// リンクのコンテンツをタイトルと説明で照らすのと同じ扱い (→ docs/search.md「リンクの一覧のファイルの中」)。
async fn search_links(
    state: &AppState,
    sources: Vec<LinksSource>,
    terms: &Terms,
) -> Result<(Vec<SearchLinkHit>, bool), AppError> {
    let paths: Vec<PathBuf> = sources.iter().map(|source| source.file.clone()).collect();
    let parsed = run_blocking(move || {
        paths
            .iter()
            .map(|path| links_file::read_path(path))
            .collect::<Vec<_>>()
    })
    .await?;

    let urls: Vec<String> = parsed
        .iter()
        .flatten()
        .flat_map(|file| file.links.iter().map(|(url, _)| url.clone()))
        .collect();
    let previews = link_preview::cached(&state.pool, &urls).await?;

    let mut hits: Vec<(String, SearchLinkHit)> = Vec::new();
    for (source, file) in sources.into_iter().zip(parsed) {
        let Some(file) = file else {
            continue;
        };
        let file_title = file
            .title
            .clone()
            .or_else(|| source.content_title.clone())
            .unwrap_or_else(|| source.file_name.clone());
        for (url, note) in file.links {
            let preview = previews.get(&url).cloned();
            let title = link_title(&url, preview.as_ref());
            let normalized_title = normalize(&title);
            let matched_in_note = if terms.matches(&[&normalized_title]) {
                false
            } else if let Some(note) = &note
                && terms.matches(&[&normalized_title, &normalize(note)])
            {
                true
            } else {
                continue;
            };
            hits.push((
                title,
                SearchLinkHit {
                    content_id: source.content_id,
                    path: source.path.clone(),
                    item: source.item,
                    file_title: file_title.clone(),
                    container_title: source.container_title.clone(),
                    url,
                    note,
                    preview,
                    matched_in_note,
                },
            ));
        }
    }

    hits.sort_by(|(a_title, a), (b_title, b)| {
        title_cmp(a_title, b_title)
            .then_with(|| title_cmp(&a.file_title, &b.file_title))
            .then_with(|| a.url.cmp(&b.url))
    });
    let truncated = hits.len() > RESULT_LIMIT;
    hits.truncate(RESULT_LIMIT);
    Ok((hits.into_iter().map(|(_, hit)| hit).collect(), truncated))
}

/// 画面のカードと同じ題。覚えているページのタイトル、無ければ、開き方の決まるファイルならファイル名、ほかはホスト名。
fn link_title(url: &str, preview: Option<&LinkPreview>) -> String {
    if let Some(title) = preview.and_then(|preview| preview.title.as_deref())
        && !title.trim().is_empty()
    {
        return title.to_string();
    }
    if let Some(file_name) = super::remote_file::relayed_file_name(url) {
        return file_name;
    }
    url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview(title: Option<&str>) -> LinkPreview {
        LinkPreview {
            title: title.map(str::to_string),
            description: None,
            site_name: None,
            published_at: None,
            image_url: None,
            icon_url: None,
        }
    }

    #[test]
    fn link_title_falls_back_to_the_host_like_the_card() {
        assert_eq!(
            link_title("https://example.com/a", Some(&preview(Some("記事")))),
            "記事"
        );
        assert_eq!(
            link_title("https://example.com/a", Some(&preview(Some(" ")))),
            "example.com"
        );
        assert_eq!(link_title("https://example.com/a", None), "example.com");
        assert_eq!(
            link_title("https://example.com/%E7%AC%AC1%E5%9B%9E.mp3", None),
            "第1回.mp3"
        );
    }
}
