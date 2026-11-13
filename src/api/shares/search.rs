//! 検索の、共有の中 (→ docs/search.md「共有の中」)。
//!
//! フォルダーの中 (`api::folder_search`) と同じく索引を持たず、検索のたびに辿る。
//! 見える範囲は、ログインした人が見られる (`read` 以上の) 共有で決まる。

use std::collections::HashSet;
use std::path::PathBuf;

use serde::Serialize;
use utoipa::ToSchema;

use crate::api::contents::{self, FolderEntry};
use crate::api::folder_search::{FOLDER_WALK_LIMIT, score_walked};
use crate::api::roots::{self, OwnDirs, Root};
use crate::api::search::{RESULT_LIMIT, Score, Terms, normalize};
use crate::api::sort::title_cmp;
use crate::auth::AuthUser;
use crate::error::{AppError, run_blocking};
use crate::state::AppState;

/// 共有の中を範囲にした検索の、確かめ済みの場所。
pub(in crate::api) struct ShareScope {
    root: WalkRoot,
}

impl ShareScope {
    /// 切り替えに出す範囲の名前。階層は画面が URL から足す。
    pub(in crate::api) fn title(&self) -> &str {
        &self.root.name
    }
}

/// 範囲にする共有の中の場所を確かめる。見られない・フォルダーでなければ 404。
pub(in crate::api) async fn scope(
    state: &AppState,
    user: &AuthUser,
    root_id: i64,
    rel: &str,
) -> Result<ShareScope, AppError> {
    // 見られなければ、ここで 404 になる。
    let resolved = super::paths::resolve_existing(state, user, root_id, rel).await?;
    let dir = resolved.canonical.clone();
    if !run_blocking(move || dir.is_dir()).await? {
        return Err(AppError::NotFound);
    }
    Ok(ShareScope {
        root: WalkRoot {
            id: resolved.root_id,
            name: resolved.root_name,
            path: resolved.root_path,
            sub_path: resolved.rel,
        },
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

/// 辿って当たったもの。ほかの区画に出たものを外してから、`finish` で行にする。
#[derive(Default)]
pub(in crate::api) struct ShareWalk {
    /// 辿った共有の id と名前。`Candidate::root` の添え字で指す。
    shares: Vec<(i64, String)>,
    candidates: Vec<Candidate>,
    incomplete: bool,
}

impl ShareWalk {
    /// 当たりが1つも無いか。
    pub(in crate::api) fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }
}

/// `user` が見られる共有の中のファイル・ディレクトリを名前で照らす。`scope` があれば、その中だけを辿る。
pub(in crate::api) async fn walk(
    state: &AppState,
    user: &AuthUser,
    scope: Option<ShareScope>,
    terms: &Terms,
) -> Result<ShareWalk, AppError> {
    let walk_roots = match scope {
        Some(scope) => vec![scope.root],
        None => outermost(roots::load_roots_for(state, user).await?),
    };
    let own_dirs = state.own_dirs.clone();
    let mut terms = terms.clone();
    let shares = walk_roots
        .iter()
        .map(|root| (root.id, root.name.clone()))
        .collect();

    // 辿るのはディスクの I/O で、共有の大きさに比例して重くなる。
    let (candidates, incomplete) = run_blocking(move || {
        let own_dirs = OwnDirs::resolve(&own_dirs);
        let mut candidates = Vec::new();
        let mut incomplete = false;
        for (index, root) in walk_roots.iter().enumerate() {
            let places = [normalize(&root.name)];
            let (entries, cut) = crate::api::fs::walk_entries(
                &root.path.join(&root.sub_path),
                FOLDER_WALK_LIMIT,
                &own_dirs,
            );
            incomplete |= cut;
            for entry in entries {
                // 結果のパスは、範囲の階層からでなく、共有からの相対にする。
                let rel_path = crate::api::fs::join_rel(&root.sub_path, &entry.rel_path);
                let name = crate::api::fs::rel_file_name(&rel_path).to_string();
                if let Some(score) = score_walked(&mut terms, &places, &rel_path, &name) {
                    candidates.push(Candidate {
                        score,
                        root: index,
                        absolute: root.path.join(&rel_path),
                        rel_path,
                        name,
                    });
                }
            }
        }
        (candidates, incomplete)
    })
    .await?;
    Ok(ShareWalk {
        shares,
        candidates,
        incomplete,
    })
}

/// 当たりを並べて打ち切り、行にする。`covered` (実体のパス) は、ほかの区画にもう出ているもの。
/// 同じものを2度並べないため、ここでは外す。
pub(in crate::api) async fn finish(
    walk: ShareWalk,
    covered: HashSet<PathBuf>,
) -> Result<ShareSearch, AppError> {
    let ShareWalk {
        shares,
        mut candidates,
        incomplete,
    } = walk;
    candidates.retain(|candidate| !covered.contains(&candidate.absolute));
    candidates.sort_by(|a, b| {
        a.score
            .cmp(&b.score)
            .then_with(|| title_cmp(&a.name, &b.name))
            .then_with(|| title_cmp(&shares[a.root].1, &shares[b.root].1))
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
}
