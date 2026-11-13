//! 検索の、共有の中 (→ docs/search.md「共有の中」)。
//!
//! フォルダーの中 (`api::folder_search`) と同じく索引を持たず、検索のたびに辿る。
//! 見える範囲は、ログインした人が見られる (`read` 以上の) 共有で決まる。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use utoipa::ToSchema;

use super::ShareAccess;
use crate::api::contents::{self, FolderEntry};
use crate::api::folder_search::FOLDER_WALK_LIMIT;
use crate::api::roots::{self, OwnDirs, Root};
use crate::api::search::{FieldKind, RESULT_LIMIT, Score, Terms, normalize};
use crate::api::sort::title_cmp;
use crate::auth::AuthUser;
use crate::error::{AppError, run_blocking};
use crate::state::AppState;

/// 共有の中を範囲にした検索の、確かめ済みの場所。
pub(in crate::api) struct ShareScope {
    root_id: i64,
    root_name: String,
    canonical: PathBuf,
    /// 共有からの相対パス (実体の綴り)。共有そのものなら空。
    rel: String,
}

impl ShareScope {
    /// 切り替えに出す範囲の名前。階層は画面が URL から足す。
    pub(in crate::api) fn title(&self) -> &str {
        &self.root_name
    }
}

/// 範囲にする共有の中の場所を確かめる。見られなければ 404、フォルダーでなければ 404。
pub(in crate::api) async fn scope(
    state: &AppState,
    user: &AuthUser,
    root_id: i64,
    rel: &str,
) -> Result<ShareScope, AppError> {
    let resolved = super::paths::resolve_existing(state, user, root_id, rel).await?;
    if resolved.access < ShareAccess::Read {
        return Err(AppError::NotFound);
    }
    let dir = resolved.canonical.clone();
    if !run_blocking(move || dir.is_dir()).await? {
        return Err(AppError::NotFound);
    }
    Ok(ShareScope {
        root_id: resolved.root_id,
        root_name: resolved.root_name,
        canonical: resolved.canonical,
        rel: resolved.rel,
    })
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(in crate::api) struct SearchShareHit {
    pub(in crate::api) share_id: i64,
    /// 行の2段目に、どこにあるかとして出す。
    pub(in crate::api) share_name: String,
    /// 共有からの相対パス (`/` 区切り)。エントリ自身の名前まで含む。
    pub(in crate::api) path: String,
    pub(in crate::api) entry: FolderEntry,
}

#[derive(Default)]
pub(in crate::api) struct ShareSearch {
    pub(in crate::api) hits: Vec<SearchShareHit>,
    pub(in crate::api) truncated: bool,
    /// 辿るエントリ数の上限で、辿りきれなかった共有があるか。
    pub(in crate::api) incomplete: bool,
}

/// 辿る1か所。
struct WalkRoot {
    id: i64,
    name: String,
    /// 共有の実体のパス (canonicalize 済み)。
    path: PathBuf,
    /// 辿り始める階層 (共有からの相対パス)。共有全体なら空。
    sub_path: String,
}

/// 当たったエントリ。行の形にするのは打ち切った後の分だけ。
struct Candidate {
    score: Score,
    root: usize,
    rel_path: String,
    name: String,
    absolute: PathBuf,
}

/// `user` が見られる共有の中のファイル・ディレクトリを名前で探す。`scope` があれば、その中だけを探す。
/// `covered` (実体のパス) は、ほかの区画にもう出ているもの。同じファイルを2度並べないため、ここでは外す。
pub(in crate::api) async fn search(
    state: &AppState,
    user: &AuthUser,
    scope: Option<ShareScope>,
    terms: &Terms,
    covered: HashSet<PathBuf>,
) -> Result<ShareSearch, AppError> {
    let walk_roots = match scope {
        // 範囲の場所から辿り、結果のパスは共有からの相対にする。
        Some(scope) => vec![WalkRoot {
            id: scope.root_id,
            name: scope.root_name,
            path: strip_rel(&scope.canonical, &scope.rel),
            sub_path: scope.rel,
        }],
        None => outermost(roots::load_roots_for(state, user).await?),
    };
    let own_dirs = state.own_dirs.clone();
    let mut terms = terms.clone();
    let shares: Vec<(i64, String)> = walk_roots
        .iter()
        .map(|root| (root.id, root.name.clone()))
        .collect();
    let names: Vec<String> = shares.iter().map(|(_, name)| name.clone()).collect();

    // 辿るのはディスクの I/O で、共有の大きさに比例して重くなる。
    let (mut candidates, incomplete) = run_blocking(move || {
        let own_dirs = OwnDirs::resolve(&own_dirs);
        let mut candidates = Vec::new();
        let mut incomplete = false;
        for (index, root) in walk_roots.iter().enumerate() {
            let share_place = normalize(&root.name);
            let (entries, cut) = crate::api::fs::walk_entries(
                &root.path.join(&root.sub_path),
                FOLDER_WALK_LIMIT,
                &own_dirs,
            );
            incomplete |= cut;
            for entry in entries {
                let rel_path = crate::api::fs::join_rel(&root.sub_path, &entry.rel_path);
                let absolute = root.path.join(&rel_path);
                if covered.contains(&absolute) {
                    continue;
                }
                let name = crate::api::fs::rel_file_name(&rel_path).to_string();
                let own = normalize(&name);
                // 途中のフォルダー名。名前の手前までの段 (共有からの相対)。
                let dirs: Vec<String> = rel_path
                    .rsplit_once('/')
                    .map(|(dirs, _)| dirs.split('/').map(normalize).collect())
                    .unwrap_or_default();
                let mut fields = vec![(FieldKind::Own, own.as_str())];
                fields.extend(
                    std::iter::once(&share_place)
                        .chain(&dirs)
                        .map(|text| (FieldKind::Place, text.as_str())),
                );
                if let Some(found) = terms.score(&fields) {
                    candidates.push(Candidate {
                        score: found.score,
                        root: index,
                        rel_path,
                        name,
                        absolute,
                    });
                }
            }
        }
        (candidates, incomplete)
    })
    .await?;

    candidates.sort_by(|a, b| {
        a.score
            .cmp(&b.score)
            .then_with(|| title_cmp(&a.name, &b.name))
            .then_with(|| title_cmp(&names[a.root], &names[b.root]))
            .then_with(|| a.rel_path.cmp(&b.rel_path))
    });
    let truncated = candidates.len() > RESULT_LIMIT;
    candidates.truncate(RESULT_LIMIT);
    let hits = run_blocking(move || {
        candidates
            .into_iter()
            .filter_map(|candidate| {
                let entry = contents::folder_entry(candidate.name, &candidate.absolute)?;
                let (share_id, share_name) = &shares[candidate.root];
                Some(SearchShareHit {
                    share_id: *share_id,
                    share_name: share_name.clone(),
                    path: candidate.rel_path,
                    entry,
                })
            })
            .collect::<Vec<_>>()
    })
    .await?;
    Ok(ShareSearch {
        hits,
        truncated,
        incomplete,
    })
}

/// `canonical` (共有の中の場所) から、共有からの相対パス `rel` の分を落とした、共有の実体のパス。
fn strip_rel(canonical: &Path, rel: &str) -> PathBuf {
    let depth = if rel.is_empty() {
        0
    } else {
        rel.split('/').count()
    };
    let mut path = canonical.to_path_buf();
    for _ in 0..depth {
        path.pop();
    }
    path
}

/// 見られる共有のうち、ほかの見られる共有に含まれないもの。入れ子の内側は外側を辿れば含まれ、
/// 両方を辿ると同じファイルが2度当たるため。同じ場所の共有が2つあれば、名前の順で先のものを採る。
fn outermost(readable: Vec<Root>) -> Vec<WalkRoot> {
    let mut kept: Vec<&Root> = Vec::new();
    for root in &readable {
        let inside_another = readable.iter().any(|other| {
            other.id != root.id
                && root.path.starts_with(&other.path)
                && (other.path != root.path || kept.iter().any(|kept| kept.id == other.id))
        });
        if !inside_another {
            kept.push(root);
        }
    }
    kept.into_iter()
        .map(|root| WalkRoot {
            id: root.id,
            name: root.name.clone(),
            path: root.path.clone(),
            sub_path: String::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(id: i64, path: &str) -> Root {
        Root {
            id,
            name: format!("share{id}"),
            path: PathBuf::from(path),
        }
    }

    fn ids(roots: &[WalkRoot]) -> Vec<i64> {
        roots.iter().map(|root| root.id).collect()
    }

    /// 入れ子の内側は外側を辿れば含まれるので、外側だけを辿る。隣の共有と、名前が前方一致するだけの共有は別に辿る。
    #[test]
    fn outermost_keeps_only_shares_not_inside_another() {
        let roots = vec![
            root(1, "/srv"),
            root(2, "/srv/inner"),
            root(3, "/other"),
            root(4, "/srv2"),
        ];
        assert_eq!(ids(&outermost(roots)), vec![1, 3, 4]);
    }

    /// 同じ場所の共有が2つあれば、並びの先の1つだけを辿る。
    #[test]
    fn outermost_walks_the_same_place_once() {
        let roots = vec![root(1, "/srv"), root(2, "/srv")];
        assert_eq!(ids(&outermost(roots)), vec![1]);
    }

    #[test]
    fn strip_rel_returns_the_share_itself() {
        assert_eq!(
            strip_rel(Path::new("/srv/a/b"), "a/b"),
            PathBuf::from("/srv")
        );
        assert_eq!(strip_rel(Path::new("/srv"), ""), PathBuf::from("/srv"));
    }
}
