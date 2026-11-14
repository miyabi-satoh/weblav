//! 共有の中の場所を、要求の相対パスから実体のパスに解決する。
//!
//! 共有の外・表示しないもの (→ `api::hidden`) は、どれも存在を伏せて 404 にする。
//! コンテンツの配信 (`contents::resolve_path`) と同じ規則。

use std::path::{Path as FsPath, PathBuf};

use crate::api::error_detail::ValidationDetail;
use crate::api::hidden::{Hidden, HiddenRules};
use crate::api::roots::{self, OwnDirs, Root};
use crate::auth::AuthUser;
use crate::error::{AppError, run_blocking};
use crate::state::AppState;

use super::{AccessMap, ShareAccess};

/// 名前の長さの上限 (バイト)。多くのファイルシステムが1つの名前に許す長さ (ext4・APFS・NTFS の 255) に合わせる。
/// NTFS の上限は UTF-16 の 255 単位だが、UTF-8 で数える方が厳しいので、どこでも作れる側に寄せる。
const NAME_MAX_BYTES: usize = 255;

/// 新しく作るファイル・フォルダーの名前を確かめ、NFC にそろえて返す。
///
/// NFC にそろえるのは、macOS から送られる NFD の名前と、ほかの端末から送られる NFC の名前が、
/// 見た目は同じなのに別のファイルになるのを避けるため。
/// 表示しないものに当たる名前 (既定ではドット始まり) は、置き場所によるので `ensure_placed_shown` で見る。
pub(super) fn validate_name(name: &str) -> Result<String, AppError> {
    let name = icu_normalizer::ComposingNormalizerBorrowed::new_nfc()
        .normalize(name)
        .into_owned();
    let invalid = name.trim().is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
        || name.len() > NAME_MAX_BYTES
        || (cfg!(windows) && is_invalid_on_windows(&name));
    if invalid {
        return Err(AppError::ValidationDetailed {
            message: "the name cannot be used for a file or folder".to_string(),
            detail: ValidationDetail::EntryNameInvalid,
        });
    }
    Ok(name)
}

/// Windows で作れない名前か。使えない記号・末尾のドットと空白 (黙って落とされ、別の名前になる)・
/// 予約された名前 (`CON` や `COM1.txt` のように、拡張子が付いても使えない)。
fn is_invalid_on_windows(name: &str) -> bool {
    const RESERVED: &[&str] = &["CON", "PRN", "AUX", "NUL"];
    if name.contains(['<', '>', ':', '"', '|', '?', '*']) || name.ends_with(['.', ' ']) {
        return true;
    }
    let stem = name.split('.').next().unwrap_or_default().trim_end();
    let upper = stem.to_ascii_uppercase();
    if RESERVED.contains(&upper.as_str()) {
        return true;
    }
    matches!(
        upper.as_bytes(),
        [b'C', b'O', b'M', b'1'..=b'9'] | [b'L', b'P', b'T', b'1'..=b'9']
    )
}

/// 解決した場所。
#[derive(Debug)]
pub(super) struct Resolved {
    pub(super) root_id: i64,
    pub(super) root_name: String,
    /// 共有の実体のパス。
    pub(super) root_path: PathBuf,
    /// 場所の実体のパス。
    pub(super) canonical: PathBuf,
    /// 共有からの相対パス。実体の綴りを '/' でつなぐ。共有そのものなら空。
    pub(super) rel: String,
    /// この場所で `user` ができること (入れ子の共有も見る)。
    pub(super) access: ShareAccess,
}

/// 共有 `root_id` の中の `rel` を解決する。見られない (`ShareAccess::None`) なら、共有が無いのと同じ 404。
///
/// 共有そのもので見られるかは、ファイルシステムに触る前に確かめる。触ってから弾くと、
/// 応答までの時間から中のものの有無を推し量れるため。
pub(super) async fn resolve_existing(
    state: &AppState,
    user: &AuthUser,
    root_id: i64,
    rel: &str,
) -> Result<Resolved, AppError> {
    crate::api::contents::reject_traversal_components(rel)?;
    let access = AccessMap::load(&state.pool, user).await?;
    let registered =
        sqlx::query!(r#"SELECT id as "id!", path FROM roots WHERE deleted_at IS NULL"#)
            .fetch_all(&state.pool)
            .await?;
    let root_path = registered
        .iter()
        .find(|row| row.id == root_id)
        .map(|row| row.path.clone())
        .ok_or(AppError::NotFound)?;
    let at_root = access.at_share(
        registered.iter().map(|row| (row.id, row.path.as_str())),
        &root_path,
    );
    if at_root < ShareAccess::Read {
        return Err(AppError::NotFound);
    }

    roots::ensure_reachable(state, &root_path).await?;
    let all_roots = roots::load_roots(state).await?;
    let root = all_roots
        .iter()
        .find(|root| root.id == root_id)
        .cloned()
        .ok_or(AppError::NotFound)?;
    let rel = rel.to_string();
    let canonical_root = root.path;
    let root_path = canonical_root.clone();
    let (canonical, rel) = crate::api::hidden::with_hidden(state, move |hidden| {
        resolve_under(&canonical_root, &rel, hidden)
    })
    .await?;
    Ok(Resolved {
        root_path,
        access: effective_access(&access, &all_roots, &canonical),
        root_id,
        root_name: root.name,
        canonical,
        rel,
    })
}

/// `resolve_existing` のうち、ファイルシステムに触る部分。`canonical_root` は canonicalize 済みの共有。
/// 実体のパスと、共有からの相対パス (実体の綴り) を返す。
///
/// ブロッキングI/Oを行うため、呼び出し側は `spawn_blocking` の中で呼ぶこと。
fn resolve_under(
    canonical_root: &FsPath,
    rel: &str,
    hidden: &Hidden,
) -> Result<(PathBuf, String), AppError> {
    if hidden.own_dirs().contains(canonical_root) {
        return Err(AppError::NotFound);
    }
    let canonical = crate::api::fs::resolve_within(canonical_root, rel, hidden)?;
    let rel = crate::api::fs::rel_of(canonical_root, &canonical)?;
    Ok((canonical, rel))
}

/// 新しく作るものの置き場所。`dir` は既にあるフォルダー、`target` はその中の `name` (まだ無いとは限らない)。
#[derive(Debug)]
pub(super) struct NewEntry {
    pub(super) dir: Resolved,
    pub(super) name: String,
    pub(super) target: PathBuf,
}

/// 共有 `root_id` の中のフォルダー `dir_rel` に、フォルダー `name` を作る場所を決める。
/// フォルダーでなければ 404。名前が使えなければ 422 (`entryNameInvalid`・`entryNameHidden`)。
/// 名前が既にあるかは見ない。作る側が、作るときに重なりを確かめる (確かめてから作るまでの間に作られうるため)。
pub(super) async fn resolve_parent_for_new(
    state: &AppState,
    user: &AuthUser,
    root_id: i64,
    dir_rel: &str,
    name: &str,
) -> Result<NewEntry, AppError> {
    let dir = resolve_existing(state, user, root_id, dir_rel).await?;
    let name = validate_name(name)?;
    let path = dir.canonical.clone();
    if !run_blocking(move || path.is_dir()).await? {
        return Err(AppError::NotFound);
    }
    ensure_placed_shown(
        &HiddenRules::load(state).await?,
        &dir.canonical,
        &name,
        true,
        None,
    )
    .await?;
    let target = dir.canonical.join(&name);
    Ok(NewEntry { dir, name, target })
}

/// 名前の変更・移動・削除・上書きをしてよい場所か。共有そのもの、中に別の共有か WebLAV のデータを
/// 含むものは 422 (`entryProtected`)。動かすと、共有の登録や WebLAV 自身がその場所を見失うため。
/// 中に共有の除外で隠したものを含むものも 422 (`entryContainsHidden`)。
pub(super) async fn ensure_not_protected(
    state: &AppState,
    resolved: &Resolved,
) -> Result<(), AppError> {
    let protected = protected_error;
    if resolved.rel.is_empty() {
        return Err(protected());
    }
    // 共有は登録したときの実体のパスで照らす。つながらない NAS の共有も見落とさないよう、
    // ファイルシステムには聞かない。
    let stored = crate::api::fs::stored_path(&resolved.canonical)?;
    let roots = sqlx::query_scalar!("SELECT path FROM roots WHERE deleted_at IS NULL")
        .fetch_all(&state.pool)
        .await?;
    if roots
        .iter()
        .any(|root| FsPath::new(root).starts_with(&stored))
    {
        return Err(protected());
    }
    ensure_nothing_hidden_inside(state, &resolved.canonical).await
}

/// フォルダー `dir` (実体のパス) に `name` を作る・置くとき、表示しないもの (→ `api::hidden`) にならないか。
/// 作っても誰にも見えず、消すことも動かすこともできなくなるため。
/// - `name` が当たれば 422 (`entryNameHidden`)。
/// - `src` (動かす・写す・名前を変える元) を置くと、中のものが共有の除外に当たるなら 422 (`entryContainsHidden`)。
///
/// 1つの要求でいくつも置くときは、`rules` を1回だけ読んで渡す。
pub(super) async fn ensure_placed_shown(
    rules: &HiddenRules,
    dir: &FsPath,
    name: &str,
    is_dir: bool,
    src: Option<&FsPath>,
) -> Result<(), AppError> {
    let target = dir.join(name);
    let src = src.map(FsPath::to_path_buf);
    let rules = rules.clone();
    let (name_hidden, inside_hidden) = run_blocking(move || {
        let hidden = rules.resolve();
        (
            hidden.hides_path(&target, is_dir),
            src.is_some_and(|src| hidden.hides_inside_at(&src, &target)),
        )
    })
    .await?;
    if name_hidden {
        return Err(AppError::ValidationDetailed {
            message: "the name is hidden by the hidden patterns".to_string(),
            detail: ValidationDetail::EntryNameHidden,
        });
    }
    if inside_hidden {
        return Err(contains_hidden_error());
    }
    Ok(())
}

fn contains_hidden_error() -> AppError {
    AppError::ValidationDetailed {
        message: "the entry contains something hidden by the share's exclude patterns".to_string(),
        detail: ValidationDetail::EntryContainsHidden,
    }
}

/// `canonical` の中に、動かす・写すと困るものが無いか。
/// - WebLAV のデータがあれば 422 (`entryProtected`)。コピーで、データ (DB・セッション鍵) を共有の中へ写し出さないため。
/// - 共有の除外で隠したものがあれば 422 (`entryContainsHidden`。→ `Hidden::has_share_hidden_inside`)。
pub(super) async fn ensure_nothing_hidden_inside(
    state: &AppState,
    canonical: &FsPath,
) -> Result<(), AppError> {
    // WebLAV のデータは、共有の様子を確かめる前に見る。共有の除外を読むと、つながらない NAS の共有を待つため。
    let own_dirs = state.own_dirs.clone();
    let path = canonical.to_path_buf();
    if run_blocking(move || OwnDirs::resolve(&own_dirs).any_within(&path)).await? {
        return Err(protected_error());
    }
    let canonical = canonical.to_path_buf();
    let inside = crate::api::hidden::with_hidden(state, move |hidden| {
        Ok(hidden.has_share_hidden_inside(&canonical))
    })
    .await?;
    if inside {
        return Err(contains_hidden_error());
    }
    Ok(())
}

fn protected_error() -> AppError {
    AppError::ValidationDetailed {
        message: "the entry is a shared folder or contains one".to_string(),
        detail: ValidationDetail::EntryProtected,
    }
}

/// 実体のパス `canonical` で `user` ができること。それを含む共有のうち一番強いもの
/// (→ `api::shares`)。`roots` は canonicalize 済み (→ `roots::load_roots`)。
pub(super) fn effective_access(
    access: &AccessMap,
    roots: &[Root],
    canonical: &FsPath,
) -> ShareAccess {
    access.effective(
        roots
            .iter()
            .filter(|root| canonical.starts_with(&root.path))
            .map(|root| root.id),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_invalid(name: &str) -> bool {
        matches!(
            validate_name(name),
            Err(AppError::ValidationDetailed {
                detail: ValidationDetail::EntryNameInvalid,
                ..
            })
        )
    }

    #[test]
    fn a_plain_name_is_accepted() {
        assert_eq!(
            validate_name("教材 2026.pdf").expect("通るはず"),
            "教材 2026.pdf"
        );
    }

    /// 見た目が同じ名前を1つのファイルにするため、NFD を NFC にする。
    #[test]
    fn a_name_is_normalized_to_nfc() {
        let nfd = "\u{30AB}\u{3099}"; // カ + 濁点
        assert_eq!(validate_name(nfd).expect("通るはず"), "\u{30AC}");
    }

    #[test]
    fn names_that_cannot_be_used_are_rejected() {
        for name in [
            "",
            "   ",
            ".",
            "..",
            "a/b",
            "a\\b",
            "a\0b",
            "a\nb",
            &"あ".repeat(86), // 258 バイト
        ] {
            assert!(is_invalid(name), "{name:?} は拒むはず");
        }
        assert!(!is_invalid(&"a".repeat(255)));
    }

    #[test]
    fn windows_reserved_and_trailing_names_are_detected() {
        for name in [
            "CON", "con.txt", "Com1", "LPT9.log", "NUL ", "a:b", "a?", "名前.", "名前 ",
        ] {
            assert!(
                is_invalid_on_windows(name),
                "{name:?} は Windows で使えない"
            );
        }
        for name in ["CONSOLE", "COM10", "LPT", "教材.pdf", "a.b.c"] {
            assert!(!is_invalid_on_windows(name), "{name:?} は Windows で使える");
        }
    }

    fn temp_dir(name: &str) -> crate::test_support::TempDir {
        crate::test_support::project_temp_dir("shares-paths", name)
    }

    #[test]
    fn a_path_inside_the_share_resolves_to_its_real_spelling() {
        let dir = temp_dir("inside");
        std::fs::create_dir_all(dir.join("教材").join("英検")).expect("フォルダーを作れなかった");
        let root = std::fs::canonicalize(&*dir).expect("canonicalize できなかった");
        let (canonical, rel) =
            resolve_under(&root, "教材/英検", &Hidden::for_test(&[], &[])).expect("解決できるはず");
        assert_eq!(canonical, root.join("教材").join("英検"));
        assert_eq!(rel, "教材/英検");
        let (canonical, rel) =
            resolve_under(&root, "", &Hidden::for_test(&[], &[])).expect("解決できるはず");
        assert_eq!(canonical, root);
        assert_eq!(rel, "");
    }

    /// 外へ出るリンク・ドット始まり・無いものは、どれも同じ 404。
    #[test]
    fn escapes_and_hidden_entries_are_not_found() {
        let dir = temp_dir("escapes");
        let root = dir.join("share");
        std::fs::create_dir_all(root.join(".secret")).expect("フォルダーを作れなかった");
        std::fs::create_dir_all(dir.join("outside")).expect("フォルダーを作れなかった");
        let root = std::fs::canonicalize(&root).expect("canonicalize できなかった");
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.join("outside"), root.join("link"))
            .expect("リンクを作れなかった");
        let mut cases = vec![".secret", "missing"];
        if cfg!(unix) {
            cases.push("link");
        }
        for rel in cases {
            assert!(
                matches!(
                    resolve_under(
                        &root,
                        rel,
                        &Hidden::for_test(&[], std::slice::from_ref(&root))
                    ),
                    Err(AppError::NotFound)
                ),
                "{rel} は 404 のはず"
            );
        }
    }

    #[test]
    fn the_innermost_and_outer_shares_both_count() {
        let sep = std::path::MAIN_SEPARATOR;
        let root = |id: i64, path: String| Root {
            id,
            name: path.clone(),
            path: PathBuf::from(path),
        };
        let roots = vec![
            root(1, format!("{sep}srv")),
            root(2, format!("{sep}srv{sep}教材")),
            root(3, format!("{sep}other")),
        ];
        let access = AccessMap::for_test(&[
            (1, ShareAccess::Read),
            (2, ShareAccess::Add),
            (3, ShareAccess::All),
        ]);
        let at = |path: String| effective_access(&access, &roots, FsPath::new(&path));
        assert_eq!(at(format!("{sep}srv{sep}教材{sep}英検")), ShareAccess::Add);
        assert_eq!(at(format!("{sep}srv{sep}写真")), ShareAccess::Read);
        assert_eq!(at(format!("{sep}elsewhere")), ShareAccess::None);
    }
}
