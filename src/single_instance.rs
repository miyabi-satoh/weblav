//! シングルインスタンス化。
//!
//! タスクトレイ常駐アプリはショートカットの誤操作等で多重起動されやすい。データ
//! ディレクトリ配下のロックファイルに対する OS のファイルロック(Unix: flock,
//! Windows: LockFileEx)で二重起動を検知する。ロックはプロセスが(正常終了・
//! クラッシュを問わず)終了して該当ファイルハンドルが閉じられれば OS 側で自動的に
//! 解放されるため、ロックファイル自体の掃除は不要。

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;

use crate::config;

/// シングルインスタンスロックの取得を試みる。
///
/// `File::try_lock`(Rust 1.89 で安定化)を使うため、専用の依存クレートは不要。
/// 返り値の `File` はロックの生存期間を握っている。呼び出し元はプロセス終了まで
/// これを保持し続けること(dropするとロックが解放される)。既に別プロセスが
/// ロックを保持している場合は `Ok(None)` を返す(これはエラーではない)。
pub fn acquire(path: &Path) -> std::io::Result<Option<File>> {
    // `path` がファイル名だけ(カレントディレクトリ相対)の場合、`parent()` は
    // 空文字列の `Path` を返す。`db::connect` に倣い、その場合は
    // ディレクトリ作成をスキップする(空パスに対する作成はエラーになるため)。
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        // ロックファイルの親はデータディレクトリそのもの(DB・セッション鍵と同居する)。
        config::create_owner_only_dir(parent)?;
    }
    // ロック取得だけが目的でファイルの中身は使わないため、既存の内容は保持する
    // (truncateすると、ロック取得中の別プロセスがいた場合に無意味な書き込みになる)。
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(err)) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn first_caller_acquires_and_second_is_rejected() {
        let tmp = TempDir::new("first_caller_acquires_and_second_is_rejected");
        let path = tmp.path().join("test.lock");

        let first = acquire(&path).unwrap();
        assert!(first.is_some(), "最初の取得はロックを保持できるはず");

        let second = acquire(&path).unwrap();
        assert!(
            second.is_none(),
            "ロック保持中の別ハンドルからの取得は None になるはず"
        );

        drop(first);
        let third = acquire(&path).unwrap();
        assert!(third.is_some(), "解放後は再度取得できるはず");
    }

    #[cfg(unix)]
    #[test]
    fn acquire_creates_owner_only_parent_dir() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new("acquire_creates_owner_only_parent_dir");
        // 親ディレクトリが未作成の状態から呼ぶ。既存のディレクトリでは、親を作る処理が
        // 権限を所有者のみに絞っているかを確かめられないため。
        std::fs::remove_dir_all(tmp.path()).expect("TempDir::newが作った直後のはず");
        let path = tmp.path().join("test.lock");

        let _lock = acquire(&path).unwrap();

        let mode = std::fs::metadata(tmp.path()).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o700,
            "ロックファイルの親ディレクトリは所有者のみアクセス可能であるべき"
        );
    }
}
