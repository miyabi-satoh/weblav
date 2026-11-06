//! 共有の中で動かした・名前を変えた・消したものを、DB の中の場所に映す。
//!
//! フォルダーのコンテンツは、登録先を実体のパスで (`contents.path`)、一覧の表示の索引を
//! 登録先からの相対パスで (`archive_items.rel_path`・`archive_dirs.rel_path`) 持つ。どれも綴りで
//! 照らすので、画面から動かしたものを追いかけないと、登録先が見つからなくなったり、索引の行が残ったりする。
//! ファイルを指すショートカットも先を `contents.path` で持つ。索引は持たない。
//!
//! - 動かしたものがコンテンツの登録先やショートカットの先 (かその上) なら、それを新しい場所に変える。中の相対パスは変わらない
//! - コンテンツの中で動かしたなら、相対パスの頭を付け替える。外へ出したなら、その分の行を消す
//! - 置き先に前からあった行は、先に消す
//! - 上書きで消したものがコンテンツの登録先 (かその中) なら、そのコンテンツの行も消す
//!
//! 一覧の表示の索引を走査している最中のコンテンツは、索引に触らない。索引は走査が書き直すため
//! (ここで直しても、走査が前に読んだ中身で上書きする)。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sqlx::SqliteConnection;

use crate::api::archive::delete_index_under;
use crate::api::fs::stored_path;
use crate::api::roots::rebase_content_path;
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

/// ファイルを指すショートカットの id と先。
async fn shortcut_rows(conn: &mut SqliteConnection) -> Result<Vec<(i64, PathBuf)>, AppError> {
    let rows = sqlx::query!(
        r#"SELECT id as "id!", path as "path!"
           FROM contents WHERE type = 'shortcut' AND path IS NOT NULL"#
    )
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.id, PathBuf::from(row.path)))
        .collect())
}

/// `old` (実体のパス) を `new` に動かした (名前を変えた) ことを映す。索引を直した、一覧の表示を使う
/// コンテンツを返す (中を読み直させるため)。`scanning` は、そのコンテンツの索引を走査中か。
pub(super) async fn moved(
    conn: &mut SqliteConnection,
    old: &Path,
    new: &Path,
    scanning: &(dyn Fn(i64) -> bool + Sync),
) -> Result<Vec<i64>, AppError> {
    let old = stored_path(old)?;
    let new = stored_path(new)?;
    if old == new {
        return Ok(Vec::new());
    }
    let mut changed = Vec::new();
    for row in folder_rows(&mut *conn).await? {
        let index_kept = scanning(row.id);
        if rebase_content_path(&mut *conn, row.id, &row.path, &old, &new).await? {
            // 登録先ごと動いたものは、登録先を付け替えるだけで中の索引はそのまま使える。
        } else if let Ok(rel_old) = rel_under(&row.path, &old) {
            if !index_kept {
                match rel_under(&row.path, &new) {
                    Ok(rel_new) => {
                        delete_index_under(&mut *conn, row.id, &rel_new).await?;
                        rename_under(&mut *conn, row.id, &rel_old, &rel_new).await?;
                    }
                    Err(_) => delete_index_under(&mut *conn, row.id, &rel_old).await?,
                }
            }
        } else if let Ok(rel_new) = rel_under(&row.path, &new) {
            // 外から入れてきたもの。同じ場所に前からあったものの行を消す。
            if !index_kept {
                delete_index_under(&mut *conn, row.id, &rel_new).await?;
            }
        } else {
            continue;
        }
        if row.list_view {
            changed.push(row.id);
        }
    }
    for (id, path) in shortcut_rows(&mut *conn).await? {
        rebase_content_path(&mut *conn, id, &path, &old, &new).await?;
    }
    Ok(changed)
}

/// `path` (実体のパス) を消したことを映す。中の索引を消す。
///
/// 消したものがコンテンツの登録先 (かその上) なら、コンテンツはそのまま残す。ごみ箱から戻せば
/// また開けるようにするため (消す前に、消すと開けなくなるコンテンツを画面で伝える)。
pub(super) async fn deleted(
    conn: &mut SqliteConnection,
    path: &Path,
    scanning: &(dyn Fn(i64) -> bool + Sync),
) -> Result<Vec<i64>, AppError> {
    let path = stored_path(path)?;
    let mut changed = Vec::new();
    for row in folder_rows(&mut *conn).await? {
        let Ok(rel) = rel_under(&row.path, &path) else {
            continue;
        };
        if !scanning(row.id) {
            delete_index_under(&mut *conn, row.id, &rel).await?;
        }
        if row.list_view {
            changed.push(row.id);
        }
    }
    Ok(changed)
}

/// `path` (実体のパス) を上書きで置き換えたことを映す。
///
/// 登録先が `path` かその中のコンテンツは、索引を消す。登録先ごと別のものになるため。
/// 登録先が `path` の外のコンテンツは、`path` の分の索引だけを消して読み直させる。
pub(super) async fn overwritten(
    conn: &mut SqliteConnection,
    path: &Path,
    scanning: &(dyn Fn(i64) -> bool + Sync),
) -> Result<Vec<i64>, AppError> {
    let mut changed = Vec::new();
    let path = stored_path(path)?;
    for row in folder_rows(&mut *conn).await? {
        let index_kept = scanning(row.id);
        if row.path.starts_with(&path) {
            if !index_kept {
                delete_all(&mut *conn, row.id).await?;
                if row.list_view {
                    // 索引のフォルダーが1つも無いと、まだ走査していないものとして読み直しが何もしない。
                    // 更新日時の分からない根を置いて、置き換わった中身を読ませる。
                    sqlx::query!(
                        "INSERT INTO archive_dirs (archive_id, rel_path, modified_ns) VALUES (?, '', NULL)",
                        row.id
                    )
                    .execute(&mut *conn)
                    .await?;
                }
            }
        } else if let Ok(rel) = rel_under(&row.path, &path) {
            if !index_kept {
                delete_index_under(&mut *conn, row.id, &rel).await?;
            }
        } else {
            continue;
        }
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
    crate::api::fs::rel_of(root, path)
}

/// コンテンツの索引をすべて消す。
async fn delete_all(conn: &mut SqliteConnection, id: i64) -> Result<(), AppError> {
    sqlx::query!("DELETE FROM archive_items WHERE archive_id = ?", id)
        .execute(&mut *conn)
        .await?;
    sqlx::query!("DELETE FROM archive_dirs WHERE archive_id = ?", id)
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
) -> Result<(), AppError> {
    let old_prefix = format!("{old}/");
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
    Moved {
        from: PathBuf,
        to: PathBuf,
    },
    Deleted(PathBuf),
    /// 上書きで消した (同じ場所に別のものを置く)。動かしたものの `Moved` より先に映すこと。
    Overwritten(PathBuf),
}

/// `changes` を DB に映し、索引を直したコンテンツの中を裏で読み直させる。
///
/// ファイルはもう動かしてあるので、映すのに失敗してもログに残すだけにする (操作を失敗に見せると、
/// 済んだ操作をもう一度させることになるため)。索引のずれは次の再スキャンで直る。
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
            Change::Overwritten(path) => overwritten(&mut tx, path, &scanning).await?,
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
    let dir = stored_path(dir)?;
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
pub(in crate::api) mod tests {
    use super::*;

    /// 区切り文字で始まる絶対パスを組む。OS の区切りで比べるテストに使う。
    pub(in crate::api) fn sep_path(parts: &[&str]) -> PathBuf {
        let mut path = PathBuf::from(std::path::MAIN_SEPARATOR_STR);
        for part in parts {
            path.push(part);
        }
        path
    }

    pub(in crate::api) async fn insert_folder(
        pool: &sqlx::SqlitePool,
        path: &Path,
        list_view: bool,
    ) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO contents (type, title, path, visibility, list_view) VALUES ('folder', 'f', ?, 'public', ?) RETURNING id",
        )
        .bind(path.display().to_string())
        .bind(list_view)
        .fetch_one(pool)
        .await
        .expect("フォルダーを作れなかった")
    }

    async fn insert_shortcut(pool: &sqlx::SqlitePool, path: &Path) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO contents (type, title, path, visibility) VALUES ('shortcut', 's', ?, 'public') RETURNING id",
        )
        .bind(path.display().to_string())
        .fetch_one(pool)
        .await
        .expect("ショートカットを作れなかった")
    }

    async fn insert_rows(pool: &sqlx::SqlitePool, id: i64, rels: &[&str]) {
        for rel in rels {
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

    async fn item_rels(pool: &sqlx::SqlitePool, id: i64) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT rel_path FROM archive_items WHERE archive_id = ? ORDER BY rel_path",
        )
        .bind(id)
        .fetch_all(pool)
        .await
        .expect("索引を読めなかった")
    }

    /// 索引と索引のフォルダーの相対パス。2つの表で同じになるはずなので、違えば落とす。
    async fn rels(pool: &sqlx::SqlitePool, id: i64) -> Vec<String> {
        let items = item_rels(pool, id).await;
        let dirs: Vec<String> = sqlx::query_scalar(
            "SELECT rel_path FROM archive_dirs WHERE archive_id = ? ORDER BY rel_path",
        )
        .bind(id)
        .fetch_all(pool)
        .await
        .expect("索引のフォルダーを読めなかった");
        assert_eq!(items, dirs, "索引と索引のフォルダーがずれた");
        items
    }

    pub(in crate::api) async fn content_path(pool: &sqlx::SqlitePool, id: i64) -> String {
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

    /// 走査中のコンテンツの索引には触らない。
    #[sqlx::test]
    async fn a_folder_being_scanned_keeps_its_index(pool: sqlx::SqlitePool) {
        let root = sep_path(&["srv", "教材"]);
        let id = insert_folder(&pool, &root, true).await;
        insert_rows(&pool, id, &["a/x.pdf"]).await;

        let mut conn = pool.acquire().await.expect("つなげなかった");
        moved(&mut conn, &root.join("a"), &root.join("c"), &|_| true)
            .await
            .expect("映せなかった");
        drop(conn);

        assert_eq!(rels(&pool, id).await, vec!["a/x.pdf"]);
    }

    /// 先のファイルの名前を変えると、先が付いていく。読み直させる索引は無い。
    #[sqlx::test]
    async fn renaming_a_file_repoints_its_shortcut(pool: sqlx::SqlitePool) {
        let root = sep_path(&["srv", "教材"]);
        let id = insert_shortcut(&pool, &root.join("a.pdf")).await;

        let changed = run_moved(&pool, &root.join("a.pdf"), &root.join("b.pdf")).await;

        assert!(changed.is_empty());
        assert_eq!(
            content_path(&pool, id).await,
            root.join("b.pdf").display().to_string()
        );
    }

    /// 大文字・小文字だけの名前の変更でも、先が付いていく。
    #[sqlx::test]
    async fn a_case_only_rename_repoints_a_shortcut(pool: sqlx::SqlitePool) {
        let root = sep_path(&["srv", "教材"]);
        let id = insert_shortcut(&pool, &root.join("a.pdf")).await;

        run_moved(&pool, &root.join("a.pdf"), &root.join("A.pdf")).await;

        assert_eq!(
            content_path(&pool, id).await,
            root.join("A.pdf").display().to_string()
        );
    }

    /// 先が入っているフォルダーを動かすと、中の相対パスを保って先が付いていく。
    #[sqlx::test]
    async fn moving_a_parent_folder_repoints_shortcuts_inside(pool: sqlx::SqlitePool) {
        let root = sep_path(&["srv", "教材"]);
        let id = insert_shortcut(&pool, &root.join("過去問").join("2026").join("a.pdf")).await;

        run_moved(
            &pool,
            &root.join("過去問"),
            &root.join("控え").join("過去問"),
        )
        .await;

        assert_eq!(
            content_path(&pool, id).await,
            root.join("控え")
                .join("過去問")
                .join("2026")
                .join("a.pdf")
                .display()
                .to_string()
        );
    }

    /// 頭の似た別のファイルや、頭の似たフォルダーの中のものは動かさない。
    #[sqlx::test]
    async fn a_shortcut_to_a_similar_sibling_is_left_alone(pool: sqlx::SqlitePool) {
        let root = sep_path(&["srv", "教材"]);
        let similar_file = root.join("a.pdf.bak");
        let similar_dir = root.join("ab").join("x.pdf");
        let file_id = insert_shortcut(&pool, &similar_file).await;
        let dir_id = insert_shortcut(&pool, &similar_dir).await;

        run_moved(&pool, &root.join("a.pdf"), &root.join("b.pdf")).await;
        run_moved(&pool, &root.join("a"), &root.join("c")).await;

        assert_eq!(
            content_path(&pool, file_id).await,
            similar_file.display().to_string()
        );
        assert_eq!(
            content_path(&pool, dir_id).await,
            similar_dir.display().to_string()
        );
    }

    /// 外へ動かしても、フォルダーの登録先と同じく新しい場所を持つ。開けるかは配信の側の判定で決まる。
    #[sqlx::test]
    async fn moving_a_file_out_repoints_its_shortcut_like_a_folder(pool: sqlx::SqlitePool) {
        let root = sep_path(&["srv", "教材"]);
        let outside = sep_path(&["srv", "写真"]);
        let folder = insert_folder(&pool, &root.join("d"), false).await;
        let shortcut = insert_shortcut(&pool, &root.join("a.pdf")).await;

        run_moved(&pool, &root.join("d"), &outside.join("d")).await;
        run_moved(&pool, &root.join("a.pdf"), &outside.join("a.pdf")).await;

        assert_eq!(
            content_path(&pool, folder).await,
            outside.join("d").display().to_string()
        );
        assert_eq!(
            content_path(&pool, shortcut).await,
            outside.join("a.pdf").display().to_string()
        );
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

    /// 上書きでは、登録先そのものやその中を登録先にするコンテンツの行も消す。上の階層のコンテンツは、
    /// 上書きした分の行だけを消す。
    #[sqlx::test]
    async fn overwriting_clears_rows_of_contents_at_or_under_the_path(pool: sqlx::SqlitePool) {
        let above = sep_path(&["srv"]);
        let root = above.join("教材");
        let under = root.join("英検");
        let above_id = insert_folder(&pool, &above, true).await;
        let root_id = insert_folder(&pool, &root, true).await;
        let under_id = insert_folder(&pool, &under, false).await;
        insert_rows(&pool, above_id, &["keep.pdf", "教材/a.pdf"]).await;
        insert_rows(&pool, root_id, &["a.pdf"]).await;
        insert_rows(&pool, under_id, &["b.pdf"]).await;

        let mut conn = pool.acquire().await.expect("つなげなかった");
        let mut changed = overwritten(&mut conn, &root, &not_scanning)
            .await
            .expect("映せなかった");
        drop(conn);

        changed.sort_unstable();
        assert_eq!(changed, vec![above_id, root_id]);
        assert_eq!(item_rels(&pool, above_id).await, vec!["keep.pdf"]);
        assert!(item_rels(&pool, root_id).await.is_empty());
        let root_dirs: Vec<String> =
            sqlx::query_scalar("SELECT rel_path FROM archive_dirs WHERE archive_id = ?")
                .bind(root_id)
                .fetch_all(&pool)
                .await
                .expect("索引のフォルダーを読めなかった");
        assert_eq!(
            root_dirs,
            vec![""],
            "読み直しが何もしないまま空にならないように"
        );
        assert!(rels(&pool, under_id).await.is_empty());
        assert_eq!(
            content_path(&pool, root_id).await,
            root.display().to_string()
        );
    }
}
