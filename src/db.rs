use std::path::Path;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use crate::config::create_owner_only_dir;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to connect to database")]
    Connect(#[source] sqlx::Error),
    #[error("failed to run migrations")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("database operation failed")]
    Sqlx(#[from] sqlx::Error),
}

/// `path` のSQLiteファイルに接続する。ファイルが無ければ作成する。
/// WAL(Write-Ahead Logging)にしているのは、タスクトレイの常駐プロセスと複数の
/// ブラウザータブなど、複数の接続が同時に読み書きし得るため
/// (デフォルトのDELETEモードだと書き込み中に読み取りがブロックされやすい)。
pub async fn connect(path: &Path) -> Result<SqlitePool, Error> {
    if let Some(parent) = path.parent() {
        create_owner_only_dir(parent).map_err(|err| Error::Connect(sqlx::Error::Io(err)))?;
    }

    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .map_err(Error::Connect)?;

    // 親ディレクトリを0700にしているため、他ユーザーからはそもそも辿れないが、
    // DBファイル自体(Argon2ハッシュ・セッションデータを含む)も多層防御として0600にする。
    // WAL/SHMファイルは親ディレクトリの権限で保護される想定でここでは対象外とする。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(err) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            tracing::warn!(%err, path = %path.display(), "failed to set database file permissions to 0600");
        }
    }

    Ok(pool)
}

/// 書き込むトランザクションを始める。
///
/// ADR: `BEGIN IMMEDIATE` で始め、最初に書き込みロックを取る。既定の `BEGIN` だと
/// 読み取りから書き込みへ移る時点でロックを取りに行き、別の書き込みと重なると
/// `busy_timeout` を待たずに `database is locked` になる (WAL でも同じ)。
/// 最初に取れば、重なっても `busy_timeout` の間は待ってから進む。
pub async fn begin_write(
    pool: &SqlitePool,
) -> Result<sqlx::Transaction<'static, sqlx::Sqlite>, sqlx::Error> {
    pool.begin_with("BEGIN IMMEDIATE").await
}

/// この版が持つマイグレーション (`migrations/`)。
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// 未適用のマイグレーションを実行する。
pub async fn migrate(pool: &SqlitePool) -> Result<(), Error> {
    MIGRATOR.run(pool).await?;
    Ok(())
}

/// この版の DB の形の版 (最後のマイグレーションの番号)。
pub fn schema_version() -> i64 {
    MIGRATOR
        .iter()
        .map(|migration| migration.version)
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    // `#[sqlx::test]` は `migrations/` を自動適用した新規DBを渡してくる。
    #[sqlx::test]
    async fn migration_creates_users_table(pool: SqlitePool) {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'users'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1);
    }

    #[sqlx::test]
    async fn migrate_is_idempotent(pool: SqlitePool) {
        migrate(&pool)
            .await
            .expect("re-running migrate should be a no-op, not an error");
    }

    #[tokio::test]
    async fn connect_creates_parent_dir_and_file() {
        let tmp = TempDir::new("connect_creates_parent_dir_and_file");
        let path = tmp.path().join("nested").join("test.db");

        let pool = connect(&path).await.expect("connect should succeed");
        migrate(&pool).await.expect("migrate should succeed");
        pool.close().await;

        assert!(path.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn connect_restricts_dir_and_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new("connect_restricts_dir_and_file_permissions");
        let path = tmp.path().join("nested").join("test.db");

        let pool = connect(&path).await.expect("connect should succeed");
        pool.close().await;

        let dir_mode = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(dir_mode & 0o777, 0o700, "{:o}", dir_mode);

        let file_mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(file_mode & 0o777, 0o600, "{:o}", file_mode);
    }
}
