//! 共有の中で動かした・名前を変えた・消したものを、DB の中の場所に映す。
//!
//! フォルダーのコンテンツは、登録先を実体のパスで (`contents.path`)、その中の公開・非公開の印と
//! 一覧の表示の索引を登録先からの相対パスで (`folder_marks.path`・`archive_items.rel_path`・
//! `archive_dirs.rel_path`) 持つ。どれも綴りで照らすので、画面から動かしたものを追いかけないと、
//! 登録先が見つからなくなったり、印が別のものに付いたり外れたりする。
//!
//! - 動かしたものがコンテンツの登録先 (かその上) なら、登録先を新しい場所に変える。中の相対パスは変わらない
//! - コンテンツの中で動かしたなら、相対パスの頭を付け替える。外へ出したなら、その分の行を消す
//! - 置き先に前からあった行 (上書きで消したものの印など) は、先に消す
//!
//! 一覧の表示の索引を走査している最中のコンテンツは、印だけを直す。索引は走査が書き直すため
//! (ここで直しても、走査が前に読んだ中身で上書きする)。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sqlx::SqliteConnection;

use crate::api::fs::path_to_string;
use crate::error::AppError;
use crate::state::AppState;

/// フォルダーのコンテンツの登録先。
struct FolderRow {
    id: i64,
    path: PathBuf,
    list_view: bool,
}

async fn folder_rows(conn: &mut SqliteConnection) -> Result<Vec<FolderRow>, AppError> {
    let rows = sqlx::query!(
        r#"SELECT id as "id!", path as "path!", list_view as "list_view: bool"
           FROM contents WHERE type = 'folder' AND path IS NOT NULL"#
    )
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| FolderRow {
            id: row.id,
            path: PathBuf::from(row.path),
            list_view: row.list_view,
        })
        .collect())
}

/// `contents.path` と同じ書き方にそろえる。Windows の canonicalize が付ける `\\?\` を落とす
/// (落とさないと、保存してあるパスと頭が合わない)。
fn stored_form(path: &Path) -> Result<PathBuf, AppError> {
    path_to_string(path.to_path_buf()).map(PathBuf::from)
}

/// `old` (実体のパス) を `new` に動かした (名前を変えた) ことを映す。索引を直した、一覧の表示を使う
/// コンテンツを返す (中を読み直させるため)。`scanning` は、そのコンテンツの索引を走査中か。
pub(super) async fn moved(
    conn: &mut SqliteConnection,
    old: &Path,
    new: &Path,
    scanning: &(dyn Fn(i64) -> bool + Sync),
) -> Result<Vec<i64>, AppError> {
    let old = stored_form(old)?;
    let new = stored_form(new)?;
    if old == new {
        return Ok(Vec::new());
    }
    let mut changed = Vec::new();
    for row in folder_rows(&mut *conn).await? {
        let marks_only = scanning(row.id);
        if let Some(path) = crate::api::roots::rebase(&row.path, &old, &new) {
            let path = path_to_string(path)?;
            sqlx::query!("UPDATE contents SET path = ? WHERE id = ?", path, row.id)
                .execute(&mut *conn)
                .await?;
        } else if let Ok(rel_old) = rel_under(&row.path, &old) {
            match rel_under(&row.path, &new) {
                Ok(rel_new) => {
                    delete_under(&mut *conn, row.id, &rel_new, marks_only).await?;
                    rename_under(&mut *conn, row.id, &rel_old, &rel_new, marks_only).await?;
                }
                Err(_) => delete_under(&mut *conn, row.id, &rel_old, marks_only).await?,
            }
        } else if let Ok(rel_new) = rel_under(&row.path, &new) {
            // 外から入れてきたもの。同じ場所に前からあったものの行を消す。
            delete_under(&mut *conn, row.id, &rel_new, marks_only).await?;
        } else {
            continue;
        }
        if row.list_view {
            changed.push(row.id);
        }
    }
    Ok(changed)
}

/// `path` (実体のパス) を消したことを映す。中の印と索引を消す。
///
/// 消したものがコンテンツの登録先 (かその上) なら、コンテンツはそのまま残す。ごみ箱から戻せば
/// また開けるようにするため (消す前に、消すと開けなくなるコンテンツを画面で伝える)。
pub(super) async fn deleted(
    conn: &mut SqliteConnection,
    path: &Path,
    scanning: &(dyn Fn(i64) -> bool + Sync),
) -> Result<Vec<i64>, AppError> {
    let path = stored_form(path)?;
    let mut changed = Vec::new();
    for row in folder_rows(&mut *conn).await? {
        let Ok(rel) = rel_under(&row.path, &path) else {
            continue;
        };
        delete_under(&mut *conn, row.id, &rel, scanning(row.id)).await?;
        if row.list_view {
            changed.push(row.id);
        }
    }
    Ok(changed)
}

/// `path` が `root` の中 (`root` そのものは除く) なら、`root` からの相対パス ('/' 区切り)。
fn rel_under(root: &Path, path: &Path) -> Result<String, AppError> {
    if path == root {
        return Err(AppError::NotFound);
    }
    crate::api::folder_marks::rel_of(root, path)
}

// 頭の一致は LIKE でなく substr で照らす。LIKE は ASCII の大文字・小文字を区別しないので、
// 大文字・小文字を区別するファイルシステムで、綴りの違う隣のものまで巻き込まないため
// (→ `folder_marks::set_marks`)。`a` と `ab` を取り違えないよう、頭は `a/` で照らす。

/// `rel` とその中の行を消す。
async fn delete_under(
    conn: &mut SqliteConnection,
    id: i64,
    rel: &str,
    marks_only: bool,
) -> Result<(), AppError> {
    let prefix = format!("{rel}/");
    sqlx::query!(
        "DELETE FROM folder_marks WHERE content_id = ?1 AND (path = ?2 OR substr(path, 1, length(?3)) = ?3)",
        id,
        rel,
        prefix
    )
    .execute(&mut *conn)
    .await?;
    if marks_only {
        return Ok(());
    }
    sqlx::query!(
        "DELETE FROM archive_items WHERE archive_id = ?1 AND (rel_path = ?2 OR substr(rel_path, 1, length(?3)) = ?3)",
        id,
        rel,
        prefix
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        "DELETE FROM archive_dirs WHERE archive_id = ?1 AND (rel_path = ?2 OR substr(rel_path, 1, length(?3)) = ?3)",
        id,
        rel,
        prefix
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// `old` とその中の行の頭を `new` に付け替える。`new` の側の行は先に消してあること (一意の制約に掛かるため)。
async fn rename_under(
    conn: &mut SqliteConnection,
    id: i64,
    old: &str,
    new: &str,
    marks_only: bool,
) -> Result<(), AppError> {
    let old_prefix = format!("{old}/");
    sqlx::query!(
        "UPDATE folder_marks SET path = ?3 || substr(path, length(?2) + 1)
         WHERE content_id = ?1 AND (path = ?2 OR substr(path, 1, length(?4)) = ?4)",
        id,
        old,
        new,
        old_prefix
    )
    .execute(&mut *conn)
    .await?;
    if marks_only {
        return Ok(());
    }
    sqlx::query!(
        "UPDATE archive_items SET rel_path = ?3 || substr(rel_path, length(?2) + 1)
         WHERE archive_id = ?1 AND (rel_path = ?2 OR substr(rel_path, 1, length(?4)) = ?4)",
        id,
        old,
        new,
        old_prefix
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        "UPDATE archive_dirs SET rel_path = ?3 || substr(rel_path, length(?2) + 1)
         WHERE archive_id = ?1 AND (rel_path = ?2 OR substr(rel_path, 1, length(?4)) = ?4)",
        id,
        old,
        new,
        old_prefix
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// ファイルシステムの変更のうち、DB に映すもの。
#[derive(Debug, Clone)]
pub(super) enum Change {
    Moved { from: PathBuf, to: PathBuf },
    Deleted(PathBuf),
}

/// `changes` を DB に映し、索引を直したコンテンツの中を裏で読み直させる。
///
/// ファイルはもう動かしてあるので、映すのに失敗してもログに残すだけにする (操作を失敗に見せると、
/// 済んだ操作をもう一度させることになるため)。印がずれたままになったものは、利用者が付け直せる。
pub(super) async fn apply(state: &AppState, changes: &[Change]) {
    if changes.is_empty() {
        return;
    }
    match apply_changes(state, changes).await {
        Ok(changed) => refresh_after_change(state, changed),
        Err(err) => {
            tracing::warn!(error = %crate::error::error_chain(&err), "updating folder locations after a file operation failed");
        }
    }
}

async fn apply_changes(state: &AppState, changes: &[Change]) -> Result<BTreeSet<i64>, AppError> {
    // コンテンツの登録先の書き換えと直列化する (→ `AppState::contents_write_lock`)。
    let _write_guard = state.contents_write_lock.lock().await;
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let scans = &state.archive_scans;
    let scanning = |id: i64| scans.is_running(id);
    let mut changed = BTreeSet::new();
    for change in changes {
        let ids = match change {
            Change::Moved { from, to } => moved(&mut tx, from, to, &scanning).await?,
            Change::Deleted(path) => deleted(&mut tx, path, &scanning).await?,
        };
        changed.extend(ids);
    }
    tx.commit().await?;
    Ok(changed)
}

/// `dir` (実体のパス) の中身を表示に含む、一覧の表示を使うコンテンツ。`dir` に足したものを索引に入れさせるのに使う。
pub(super) async fn list_views_containing(
    pool: &sqlx::SqlitePool,
    dir: &Path,
) -> Result<Vec<i64>, AppError> {
    let dir = stored_form(dir)?;
    let mut conn = pool.acquire().await?;
    Ok(folder_rows(&mut conn)
        .await?
        .into_iter()
        .filter(|row| row.list_view && dir.starts_with(&row.path))
        .map(|row| row.id)
        .collect())
}

/// 一覧の表示を使うコンテンツの中を、裏で読み直させる。共有の中に置いた・動かしたものが、
/// 次の再スキャンを待たずに一覧に出るようにするため (→ `archive::refresh_changed_dirs`)。
pub(in crate::api) fn refresh_after_change(state: &AppState, ids: impl IntoIterator<Item = i64>) {
    let ids: Vec<i64> = ids.into_iter().collect();
    if ids.is_empty() {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        for id in ids {
            match crate::api::archive::load_archive(&state.pool, id).await {
                Ok(meta) => crate::api::archive::refresh_changed_dirs(&state, id, &meta).await,
                // 動かしている間に消された・一覧の表示をやめたコンテンツ。
                Err(AppError::NotFound) => {}
                Err(err) => {
                    tracing::warn!(archive_id = id, error = %crate::error::error_chain(&err), "refreshing a folder after a file operation failed");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sep_path(parts: &[&str]) -> PathBuf {
        let mut path = PathBuf::from(std::path::MAIN_SEPARATOR_STR);
        for part in parts {
            path.push(part);
        }
        path
    }

    async fn insert_folder(pool: &sqlx::SqlitePool, path: &Path, list_view: bool) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO contents (type, title, path, visibility, list_view) VALUES ('folder', 'f', ?, 'public', ?) RETURNING id",
        )
        .bind(path.display().to_string())
        .bind(list_view)
        .fetch_one(pool)
        .await
        .expect("フォルダーを作れなかった")
    }

    async fn insert_rows(pool: &sqlx::SqlitePool, id: i64, rels: &[&str]) {
        for rel in rels {
            sqlx::query(
                "INSERT INTO folder_marks (content_id, path, is_dir, public, created_at) VALUES (?, ?, 0, 1, 0)",
            )
            .bind(id)
            .bind(rel)
            .execute(pool)
            .await
            .expect("印を入れられなかった");
            sqlx::query("INSERT INTO archive_items (archive_id, rel_path) VALUES (?, ?)")
                .bind(id)
                .bind(rel)
                .execute(pool)
                .await
                .expect("索引を入れられなかった");
            sqlx::query(
                "INSERT INTO archive_dirs (archive_id, rel_path, modified_ns) VALUES (?, ?, 0)",
            )
            .bind(id)
            .bind(rel)
            .execute(pool)
            .await
            .expect("索引のフォルダーを入れられなかった");
        }
    }

    /// 印・索引・索引のフォルダーの相対パス。3つの表で同じになるはずなので、違えば落とす。
    async fn rels(pool: &sqlx::SqlitePool, id: i64) -> Vec<String> {
        let marks: Vec<String> =
            sqlx::query_scalar("SELECT path FROM folder_marks WHERE content_id = ? ORDER BY path")
                .bind(id)
                .fetch_all(pool)
                .await
                .expect("印を読めなかった");
        let items: Vec<String> = sqlx::query_scalar(
            "SELECT rel_path FROM archive_items WHERE archive_id = ? ORDER BY rel_path",
        )
        .bind(id)
        .fetch_all(pool)
        .await
        .expect("索引を読めなかった");
        let dirs: Vec<String> = sqlx::query_scalar(
            "SELECT rel_path FROM archive_dirs WHERE archive_id = ? ORDER BY rel_path",
        )
        .bind(id)
        .fetch_all(pool)
        .await
        .expect("索引のフォルダーを読めなかった");
        assert_eq!(marks, items, "印と索引がずれた");
        assert_eq!(marks, dirs, "印と索引のフォルダーがずれた");
        marks
    }

    async fn content_path(pool: &sqlx::SqlitePool, id: i64) -> String {
        sqlx::query_scalar("SELECT path FROM contents WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("登録先を読めなかった")
    }

    fn not_scanning(_: i64) -> bool {
        false
    }

    async fn run_moved(pool: &sqlx::SqlitePool, old: &Path, new: &Path) -> Vec<i64> {
        let mut conn = pool.acquire().await.expect("つなげなかった");
        moved(&mut conn, old, new, &not_scanning)
            .await
            .expect("映せなかった")
    }

    /// 中で名前を変えると、そのものと中の行の頭が変わる。`a` と頭の似た `ab` は巻き込まない。
    #[sqlx::test]
    async fn renaming_inside_a_folder_rewrites_its_rows_but_not_a_similar_sibling(
        pool: sqlx::SqlitePool,
    ) {
        let root = sep_path(&["srv", "教材"]);
        let id = insert_folder(&pool, &root, true).await;
        insert_rows(&pool, id, &["a", "a/x.pdf", "ab/y.pdf", "b.pdf"]).await;

        let changed = run_moved(&pool, &root.join("a"), &root.join("c")).await;

        assert_eq!(changed, vec![id]);
        assert_eq!(
            rels(&pool, id).await,
            vec!["ab/y.pdf", "b.pdf", "c", "c/x.pdf"]
        );
    }

    /// 置き先に前からあった行は消してから付け替える。
    #[sqlx::test]
    async fn rows_left_at_the_destination_are_replaced(pool: sqlx::SqlitePool) {
        let root = sep_path(&["srv", "教材"]);
        let id = insert_folder(&pool, &root, false).await;
        insert_rows(&pool, id, &["a/x.pdf", "c/old.pdf", "c/x.pdf"]).await;

        let changed = run_moved(&pool, &root.join("a"), &root.join("c")).await;

        // 一覧の表示を使っていなければ、読み直させるものは無い。
        assert!(changed.is_empty());
        assert_eq!(rels(&pool, id).await, vec!["c/x.pdf"]);
    }

    /// 外へ出したものの行は消え、外から入れた場所に残っていた行も消える。
    #[sqlx::test]
    async fn moving_out_drops_rows_and_moving_in_clears_stale_ones(pool: sqlx::SqlitePool) {
        let first = sep_path(&["srv", "教材"]);
        let second = sep_path(&["srv", "写真"]);
        let first_id = insert_folder(&pool, &first, true).await;
        let second_id = insert_folder(&pool, &second, true).await;
        insert_rows(&pool, first_id, &["a", "a/x.pdf", "b.pdf"]).await;
        insert_rows(&pool, second_id, &["a/stale.pdf", "keep.jpg"]).await;

        let changed = run_moved(&pool, &first.join("a"), &second.join("a")).await;

        assert_eq!(changed, vec![first_id, second_id]);
        assert_eq!(rels(&pool, first_id).await, vec!["b.pdf"]);
        assert_eq!(rels(&pool, second_id).await, vec!["keep.jpg"]);
    }

    /// コンテンツの登録先 (かその上) を動かすと、登録先が変わり、中の相対パスはそのまま。
    #[sqlx::test]
    async fn moving_a_registered_folder_repoints_the_content(pool: sqlx::SqlitePool) {
        let parent = sep_path(&["srv", "教材"]);
        let registered = parent.join("英検");
        let id = insert_folder(&pool, &registered, true).await;
        let sibling = insert_folder(&pool, &sep_path(&["srv", "教材2"]), false).await;
        insert_rows(&pool, id, &["a.pdf"]).await;

        let renamed = sep_path(&["srv", "教材 2026"]);
        let changed = run_moved(&pool, &parent, &renamed).await;

        assert_eq!(changed, vec![id]);
        assert_eq!(
            content_path(&pool, id).await,
            renamed.join("英検").display().to_string()
        );
        assert_eq!(rels(&pool, id).await, vec!["a.pdf"]);
        // 頭の似た別の登録先は動かさない。
        assert_eq!(
            content_path(&pool, sibling).await,
            sep_path(&["srv", "教材2"]).display().to_string()
        );
    }

    /// 登録先そのものの名前を変えても、登録先の綴りの末尾に区切りが付かない。
    #[sqlx::test]
    async fn renaming_a_registered_folder_itself_keeps_the_path_spelling(pool: sqlx::SqlitePool) {
        let registered = sep_path(&["srv", "教材"]);
        let id = insert_folder(&pool, &registered, false).await;

        let renamed = sep_path(&["srv", "教材 2026"]);
        run_moved(&pool, &registered, &renamed).await;

        assert_eq!(content_path(&pool, id).await, renamed.display().to_string());
    }

    /// 大文字・小文字だけの名前の変更でも、行が付け替わる。
    #[sqlx::test]
    async fn a_case_only_rename_rewrites_rows(pool: sqlx::SqlitePool) {
        let root = sep_path(&["srv", "教材"]);
        let id = insert_folder(&pool, &root, false).await;
        insert_rows(&pool, id, &["photos/a.jpg"]).await;

        run_moved(&pool, &root.join("photos"), &root.join("Photos")).await;

        assert_eq!(rels(&pool, id).await, vec!["Photos/a.jpg"]);
    }

    /// 走査中のコンテンツは印だけを直す。
    #[sqlx::test]
    async fn a_folder_being_scanned_gets_only_its_marks_rewritten(pool: sqlx::SqlitePool) {
        let root = sep_path(&["srv", "教材"]);
        let id = insert_folder(&pool, &root, true).await;
        insert_rows(&pool, id, &["a/x.pdf"]).await;

        let mut conn = pool.acquire().await.expect("つなげなかった");
        moved(&mut conn, &root.join("a"), &root.join("c"), &|_| true)
            .await
            .expect("映せなかった");
        drop(conn);

        let marks: Vec<String> =
            sqlx::query_scalar("SELECT path FROM folder_marks WHERE content_id = ?")
                .bind(id)
                .fetch_all(&pool)
                .await
                .expect("印を読めなかった");
        let items: Vec<String> =
            sqlx::query_scalar("SELECT rel_path FROM archive_items WHERE archive_id = ?")
                .bind(id)
                .fetch_all(&pool)
                .await
                .expect("索引を読めなかった");
        assert_eq!(marks, vec!["c/x.pdf"]);
        assert_eq!(items, vec!["a/x.pdf"]);
    }

    /// 消すと中の行が消える。登録先そのものを消しても、コンテンツは残す。
    #[sqlx::test]
    async fn deleting_drops_rows_inside_and_keeps_the_content(pool: sqlx::SqlitePool) {
        let root = sep_path(&["srv", "教材"]);
        let id = insert_folder(&pool, &root, true).await;
        insert_rows(&pool, id, &["a", "a/x.pdf", "ab.pdf"]).await;

        let mut conn = pool.acquire().await.expect("つなげなかった");
        let changed = deleted(&mut conn, &root.join("a"), &not_scanning)
            .await
            .expect("映せなかった");
        assert_eq!(changed, vec![id]);
        let changed = deleted(&mut conn, &root, &not_scanning)
            .await
            .expect("映せなかった");
        assert!(changed.is_empty());
        drop(conn);

        assert_eq!(rels(&pool, id).await, vec!["ab.pdf"]);
        assert_eq!(content_path(&pool, id).await, root.display().to_string());
    }
}
