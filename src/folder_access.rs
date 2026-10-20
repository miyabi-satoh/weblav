//! macOS のサンドボックスで、公開できるフォルダーを起動し直しても読めるようにする
//! (security-scoped bookmark、→ docs/distribution.md「ビルド・配布の方法」)。
//!
//! サンドボックスでは、利用者がフォルダー選択の窓で選んだフォルダーしか読めず、その許可は
//! プロセスが終わると消える。窓で選んだ直後にブックマークを作って DB に残し
//! (`folder_bookmarks`)、起動のたびに登録中のフォルダーの分だけ許可を戻す。
//! macOS 以外では何もしない。

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

use sqlx::SqlitePool;

use crate::error::run_blocking;

/// 窓で選んだ直後のフォルダーのブックマークを作る。macOS 以外は要らないので `None`。
pub fn bookmark(path: &Path) -> Result<Option<Vec<u8>>, String> {
    platform::bookmark(path)
}

/// 窓で選んだフォルダーのブックマークを残す。同じパスがあれば書き直す。
pub async fn save(pool: &SqlitePool, path: &str, bookmark: &[u8]) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO folder_bookmarks (path, bookmark) VALUES (?, ?)
         ON CONFLICT (path) DO UPDATE SET bookmark = excluded.bookmark",
        path,
        bookmark
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// 公開できるフォルダーのブックマーク。無い (macOS 以外・窓で選んでいない) か読めなければ `None`。
pub async fn load(pool: &SqlitePool, path: &str) -> Option<Vec<u8>> {
    sqlx::query_scalar!("SELECT bookmark FROM folder_bookmarks WHERE path = ?", path)
        .fetch_optional(pool)
        .await
        .unwrap_or_else(|err| {
            tracing::warn!(%path, %err, "failed to read a folder bookmark");
            None
        })
}

/// 外れた共有や外付けのディスクを、ブックマークからつなぎ直して読む許可を取り直す
/// (→ `folder_reach`)。パスワードはキーチェーンに保存したものを使い、窓は出さない。
pub fn reconnect(bookmark: &[u8]) -> Result<(), String> {
    platform::reconnect(bookmark)
}

/// このプロセスで許可を戻したフォルダー。戻すたびに start を重ねないため (対になる stop は呼ばない)。
static STARTED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// 登録中の公開フォルダーの分だけ、ブックマークから読む許可を戻す。
/// 許可はプロセスが終わるまで持ち続ける。戻せないもの (フォルダーが消えた・別の Mac の DB など) は
/// ログに残して飛ばす。そのフォルダーは、存在しないフォルダーと同じく読めないまま。
/// 失敗しても起動やバックアップからの復元は止めない。読めないフォルダーは選び直せば直るため。
pub async fn restore(pool: &SqlitePool) {
    let rows = match sqlx::query!(
        "SELECT b.path, b.bookmark FROM folder_bookmarks b
         JOIN roots r ON r.path = b.path AND r.deleted_at IS NULL"
    )
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(err) => {
            tracing::warn!(%err, "failed to read folder bookmarks");
            return;
        }
    };
    let rows: Vec<_> = {
        let started = STARTED.lock().unwrap_or_else(PoisonError::into_inner);
        rows.into_iter()
            .filter(|row| !started.contains(&row.path))
            .map(|row| (row.path, row.bookmark))
            .collect()
    };
    let resolved = run_blocking(move || {
        rows.into_iter()
            .map(|(path, bookmark)| {
                let result = platform::restore(&bookmark);
                (path, result)
            })
            .collect::<Vec<_>>()
    })
    .await;
    let resolved = match resolved {
        Ok(resolved) => resolved,
        Err(err) => {
            tracing::warn!(%err, "failed to restore folder access");
            return;
        }
    };
    for (path, result) in resolved {
        let renewed = match result {
            Ok(renewed) => renewed,
            Err(err) => {
                tracing::warn!(%path, %err, "failed to restore folder access");
                continue;
            }
        };
        STARTED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.clone());
        // 古くなったブックマーク (フォルダーが動いた等) は、戻せても作り直しを求められる。
        if let Some(renewed) = renewed
            && let Err(err) = save(pool, &path, &renewed).await
        {
            tracing::warn!(%path, %err, "failed to renew a folder bookmark");
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::path::Path;

    use objc2::rc::Retained;
    use objc2::runtime::Bool;
    use objc2_foundation::{
        NSData, NSString, NSURL, NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions,
    };

    /// 公開できるフォルダーは読むだけなので、読む許可のブックマークにする。
    fn creation_options() -> NSURLBookmarkCreationOptions {
        NSURLBookmarkCreationOptions::WithSecurityScope
            | NSURLBookmarkCreationOptions::SecurityScopeAllowOnlyReadAccess
    }

    fn create(url: &NSURL) -> Result<Vec<u8>, String> {
        url.bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
            creation_options(),
            None,
            None,
        )
        .map(|data| data.to_vec())
        .map_err(|err| format!("{err:?}"))
    }

    pub fn bookmark(path: &Path) -> Result<Option<Vec<u8>>, String> {
        let url =
            NSURL::fileURLWithPath_isDirectory(&NSString::from_str(&path.to_string_lossy()), true);
        create(&url).map(Some)
    }

    /// 解決して読む許可を戻す。戻り値は、作り直したブックマーク (古くなっていたとき)。
    fn resolve(
        bookmark: &[u8],
        options: NSURLBookmarkResolutionOptions,
    ) -> Result<Option<Vec<u8>>, String> {
        let data = NSData::with_bytes(bookmark);
        let mut stale = Bool::NO;
        // SAFETY: `stale` は呼び出しの間だけ生きている有効なポインタ。
        let url: Retained<NSURL> = unsafe {
            NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
                &data,
                NSURLBookmarkResolutionOptions::WithSecurityScope
                    | NSURLBookmarkResolutionOptions::WithoutUI
                    | options,
                None,
                &mut stale,
            )
        }
        .map_err(|err| format!("{err:?}"))?;
        // 対になる stop は呼ばない。許可はプロセスが終わるまで使う。
        // SAFETY: 解決したばかりの URL に対する呼び出しで、前提は無い。
        if !unsafe { url.startAccessingSecurityScopedResource() } {
            return Err("startAccessingSecurityScopedResource returned false".to_string());
        }
        if stale.as_bool() {
            return create(&url).map(Some);
        }
        Ok(None)
    }

    /// 起動のときは、つながっていない共有や外付けのディスクをマウントしない。
    /// つながらない NAS を待って起動が遅れないよう、つなぎ直しは使うときに回す (→ `reconnect`)。
    pub fn restore(bookmark: &[u8]) -> Result<Option<Vec<u8>>, String> {
        resolve(bookmark, NSURLBookmarkResolutionOptions::WithoutMounting)
    }

    /// 解決するとき、外れた共有をマウントし直してもらう。
    pub fn reconnect(bookmark: &[u8]) -> Result<(), String> {
        resolve(bookmark, NSURLBookmarkResolutionOptions::empty()).map(|_| ())
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use std::path::Path;

    pub fn bookmark(_path: &Path) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }

    pub fn restore(_bookmark: &[u8]) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }

    pub fn reconnect(_bookmark: &[u8]) -> Result<(), String> {
        Err("bookmarks are only used on macOS".to_string())
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    /// サンドボックスの外 (テスト・`weblav-service`) でも、作ったブックマークから戻せる。
    #[test]
    fn a_bookmark_restores_outside_the_sandbox() {
        let dir = crate::test_support::project_temp_dir("folder-access-tests", "restore");
        let bookmark = bookmark(&dir)
            .expect("ブックマークを作れるはず")
            .expect("macOS では作るはず");
        assert_eq!(platform::restore(&bookmark), Ok(None));
    }
}
