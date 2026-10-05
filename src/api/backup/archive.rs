//! バックアップの zip を作る・読む・その中身で今のデータを置き換える (→ docs/access.md「バックアップとリストア」)。
//!
//! zip の中身:
//! - `manifest.json`: 作った版と日時 (`Manifest`)
//! - `weblav.db`: DB の一貫したコピー (`VACUUM INTO`)
//! - `blobs/<hash>`: file コンテンツの実体

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection, SqlitePoolOptions};
use sqlx::{Connection, SqlitePool};
use zip::result::ZipError;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::api::error_detail::ValidationDetail;
use crate::config::create_owner_only_dir;
use crate::error::{AppError, run_blocking};
use crate::session::RevokedSessions;

/// zip の形の版。入れるものや目録の項目を変えたら上げる。
const FORMAT: u32 = 1;
const MANIFEST_ENTRY: &str = "manifest.json";
const DB_ENTRY: &str = "weblav.db";
const BLOBS_PREFIX: &str = "blobs/";
/// 目録として読む上限。目録は数百バイトなので、これを超えるものは目録ではない。
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
/// tower-sessions が作るセッションの表。戻さずに中身を消す。
pub(super) const SESSION_TABLE: &str = "tower_sessions";
/// この Mac で選んだフォルダへの読む許可 (→ docs/distribution.md「ビルド・配布の方法」)。データではなくこの PC の状態なので、
/// 戻さずに今の行を残す。
const FOLDER_BOOKMARKS_TABLE: &str = "folder_bookmarks";
/// リンクのカードの情報 (→ `api::link_preview`)。画像の実体はこの PC の置き場にあり、バックアップに入らないので、
/// 戻さずに今の行を残す。
const LINK_PREVIEWS_TABLE: &str = "link_previews";

/// zip に入れる目録。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Manifest {
    pub format: u32,
    /// 作った weblav の版。確認の画面に出す。
    pub app_version: String,
    /// 作った日時 (UNIX 秒)。確認の画面に出す。
    pub created_at: i64,
    /// 作った時点の DB の形の版 (→ `db::schema_version`)。今の版より新しいものは戻せない。
    pub schema_version: i64,
}

impl Manifest {
    pub(super) fn now() -> Self {
        Self {
            format: FORMAT,
            app_version: crate::APP_VERSION.to_string(),
            created_at: time::OffsetDateTime::now_utc().unix_timestamp(),
            schema_version: crate::db::schema_version(),
        }
    }
}

/// 受け取ったファイルをバックアップとして読めない理由。
#[derive(Debug, thiserror::Error)]
pub(super) enum ReadError {
    #[error("not a WebLAV backup")]
    Invalid,
    #[error("the backup was made by a newer version ({app_version})")]
    TooNew { app_version: String },
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl From<ReadError> for AppError {
    fn from(err: ReadError) -> Self {
        let message = err.to_string();
        match err {
            ReadError::Invalid => AppError::ValidationDetailed {
                message,
                detail: ValidationDetail::BackupInvalid,
            },
            ReadError::TooNew { app_version } => AppError::ValidationDetailed {
                message,
                detail: ValidationDetail::BackupTooNew { app_version },
            },
            ReadError::Io(err) => AppError::Io(err),
        }
    }
}

/// zip として読めないものは、バックアップではないとみなす。手元のディスクの読み書きの失敗は 5xx のままにする。
fn zip_read_error(err: ZipError) -> ReadError {
    match err {
        ZipError::Io(err) => io_read_error(err),
        _ => ReadError::Invalid,
    }
}

/// 中身を展開するときの I/O エラーのうち、壊れた圧縮データ (と途中で切れたファイル) によるもの。
fn io_read_error(err: io::Error) -> ReadError {
    match err.kind() {
        io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof => ReadError::Invalid,
        _ => ReadError::Io(err),
    }
}

/// 作業用の一時ディレクトリ。drop で中身ごと消す。
///
/// 置き場は設定とデータの置き場の中 (`AppDirs::backup_work_dir`)。DB の写しを含むので、
/// OS の一時ディレクトリには置かない。
pub(super) struct WorkDir(PathBuf);

impl WorkDir {
    pub(super) fn create(root: &Path) -> io::Result<Self> {
        create_owner_only_dir(root)?;
        let path = root.join(super::super::random_hex::<16>()?);
        create_owner_only_dir(&path)?;
        Ok(Self(path))
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

/// async の文脈で drop されることが多いので、tokio の中なら消す処理を blocking のスレッドへ回す。
impl Drop for WorkDir {
    fn drop(&mut self) {
        let path = std::mem::take(&mut self.0);
        let remove = move || {
            if let Err(err) = std::fs::remove_dir_all(&path)
                && err.kind() != io::ErrorKind::NotFound
            {
                tracing::warn!(%err, path = %path.display(), "failed to remove a backup work directory");
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => drop(handle.spawn_blocking(remove)),
            Err(_) => remove(),
        }
    }
}

/// 前に動いていたときの作業用のディレクトリを消す。途中で止まると残るため、起動したときに呼ぶ。
pub(crate) fn remove_work_root(root: &Path) {
    if let Err(err) = std::fs::remove_dir_all(root)
        && err.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(%err, path = %root.display(), "failed to remove leftover backup work files");
    }
}

/// zip に入れる DB と blob の写し。
pub(super) struct Snapshot {
    dir: WorkDir,
    blob_hashes: Vec<String>,
}

/// 今の DB と blob を写し取る。
///
/// 呼び出し側は `contents_write_lock` を持っていること。DB の写しと blob の集合を
/// 同じ時点に揃えるため (持たないと、写した後に消された blob を入れ損ねる)。
/// blob はハードリンクで写すので、鍵を持つ時間は blob の大きさによらず短い。
pub(super) async fn snapshot(
    pool: &SqlitePool,
    blobs_dir: &Path,
    work_root: &Path,
) -> Result<Snapshot, AppError> {
    let root = work_root.to_path_buf();
    let dir = run_blocking(move || WorkDir::create(&root)).await??;

    let db_path = dir.path().join(DB_ENTRY);
    let db_path = db_path
        .to_str()
        .ok_or_else(|| io::Error::other("the data directory path is not valid UTF-8"))?;
    sqlx::query!("VACUUM INTO ?", db_path).execute(pool).await?;

    let blob_hashes = referenced_blobs(pool).await?;
    let blobs_dir = blobs_dir.to_path_buf();
    let snapshot = run_blocking(move || -> io::Result<Snapshot> {
        let linked_dir = dir.path().join("blobs");
        create_owner_only_dir(&linked_dir)?;
        let mut kept = Vec::with_capacity(blob_hashes.len());
        for hash in blob_hashes {
            if !super::super::contents::is_valid_blob_hash(&hash) {
                tracing::warn!(blob_hash = %hash, "blob_hash is not 64 hex digits; leaving it out of the backup");
                continue;
            }
            let source = blobs_dir.join(&hash);
            let target = linked_dir.join(&hash);
            match std::fs::hard_link(&source, &target) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {
                    tracing::warn!(blob_hash = %hash, "blob file is missing; leaving it out of the backup");
                    continue;
                }
                // ハードリンクを張れないファイルシステムでは、写しを取る。
                Err(_) => {
                    std::fs::copy(&source, &target)?;
                }
            }
            kept.push(hash);
        }
        Ok(Snapshot {
            dir,
            blob_hashes: kept,
        })
    })
    .await??;
    Ok(snapshot)
}

async fn referenced_blobs(pool: &SqlitePool) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT DISTINCT blob_hash as "blob_hash!" FROM contents WHERE blob_hash IS NOT NULL"#
    )
    .fetch_all(pool)
    .await
}

/// `snapshot` を zip にして `out` へ書く。書き終えた `out` を返す。
pub(super) fn write_zip<W: Write>(
    snapshot: &Snapshot,
    manifest: &Manifest,
    out: W,
) -> io::Result<W> {
    let mut zip = ZipWriter::new_stream(out);
    let deflated = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .large_file(true);
    // ADR: blob は圧縮しない。画像・音声・動画・PDF は既に圧縮されていて、deflate し直しても
    // 小さくならず時間だけ掛かるため。
    let stored = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .large_file(true);

    zip.start_file(MANIFEST_ENTRY, deflated)?;
    zip.write_all(&serde_json::to_vec_pretty(manifest)?)?;

    zip.start_file(DB_ENTRY, deflated)?;
    io::copy(
        &mut File::open(snapshot.dir.path().join(DB_ENTRY))?,
        &mut zip,
    )?;

    let blobs_dir = snapshot.dir.path().join("blobs");
    for hash in &snapshot.blob_hashes {
        zip.start_file(format!("{BLOBS_PREFIX}{hash}"), stored)?;
        io::copy(&mut File::open(blobs_dir.join(hash))?, &mut zip)?;
    }

    let mut out = zip.finish()?.into_inner();
    out.flush()?;
    Ok(out)
}

/// 目録を読み、この版で戻せるバックアップかを確かめる。
pub(super) fn read_manifest(path: &Path) -> Result<Manifest, ReadError> {
    let mut archive = ZipArchive::new(File::open(path)?).map_err(zip_read_error)?;

    let mut bytes = Vec::new();
    archive
        .by_name(MANIFEST_ENTRY)
        .map_err(zip_read_error)?
        .take(MAX_MANIFEST_BYTES)
        .read_to_end(&mut bytes)
        .map_err(io_read_error)?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| ReadError::Invalid)?;

    if manifest.format > FORMAT || manifest.schema_version > crate::db::schema_version() {
        return Err(ReadError::TooNew {
            app_version: manifest.app_version,
        });
    }
    if manifest.format != FORMAT {
        return Err(ReadError::Invalid);
    }
    archive.by_name(DB_ENTRY).map_err(zip_read_error)?;
    Ok(manifest)
}

/// `zip_path` のバックアップで、今の DB と blob を置き換える。消したセッションは `revoked` に登録する。
///
/// 呼び出し側は `users_write_lock` と `contents_write_lock` を持っていること。
/// 置き換える途中で、利用者の操作や blob の後始末が割り込まないようにするため。
pub(super) async fn restore(
    pool: &SqlitePool,
    blobs_dir: &Path,
    work_root: &Path,
    zip_path: &Path,
    manifest: &Manifest,
    revoked: &RevokedSessions,
) -> Result<(), AppError> {
    let root = work_root.to_path_buf();
    let dir = run_blocking(move || WorkDir::create(&root)).await??;
    let db_path = dir.path().join(DB_ENTRY);

    // ADR: blob は今の置き場へ先に足す。同じ名前は同じ中身なので上書きの必要が無い。
    // DB を置き換える前に失敗しても、今の DB から参照されていない blob が増えるだけで、
    // それも下で消す (鍵を持っているので、アップロードの途中の blob を消すことは無い)。
    let result = async {
        let (zip, db, blobs) = (
            zip_path.to_path_buf(),
            db_path.clone(),
            blobs_dir.to_path_buf(),
        );
        run_blocking(move || extract(&zip, &db, &blobs)).await??;
        prepare_db(&db_path, manifest).await?;
        replace_tables(pool, &db_path, revoked).await
    }
    .await;
    remove_unreferenced_blobs(pool, blobs_dir).await;
    result
}

/// DB を `db_out` へ、blob を `blobs_dir` へ取り出す。blob は名前と中身のハッシュが合うものだけを置く。
fn extract(zip_path: &Path, db_out: &Path, blobs_dir: &Path) -> Result<(), ReadError> {
    let mut archive = ZipArchive::new(File::open(zip_path)?).map_err(zip_read_error)?;
    // アップロードと同じ一時ディレクトリを使う。置き場へ rename するには同じファイルシステムに要るため。
    let tmp_dir = blobs_dir.join("tmp");
    create_owner_only_dir(blobs_dir)?;
    create_owner_only_dir(&tmp_dir)?;

    let mut has_db = false;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(zip_read_error)?;
        let name = entry.name().to_owned();
        if name == DB_ENTRY {
            // 2つ目があるのは、このアプリが作ったものではない。
            if has_db {
                return Err(ReadError::Invalid);
            }
            let mut out = File::create_new(db_out)?;
            io::copy(&mut entry, &mut out).map_err(io_read_error)?;
            has_db = true;
        } else if let Some(hash) = name.strip_prefix(BLOBS_PREFIX) {
            if !super::super::contents::is_valid_blob_hash(hash) {
                return Err(ReadError::Invalid);
            }
            extract_blob(&mut entry, hash, blobs_dir, &tmp_dir)?;
        }
    }
    if has_db {
        Ok(())
    } else {
        Err(ReadError::Invalid)
    }
}

fn extract_blob(
    entry: &mut impl Read,
    hash: &str,
    blobs_dir: &Path,
    tmp_dir: &Path,
) -> Result<(), ReadError> {
    let dest = blobs_dir.join(hash);
    if dest.exists() {
        return Ok(());
    }

    let tmp = TempFile(tmp_dir.join(format!("restore-{}", super::super::random_hex::<16>()?)));
    let mut file = File::create_new(&tmp.0)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let read = entry.read(&mut buf).map_err(io_read_error)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
        file.write_all(&buf[..read])?;
    }
    drop(file);

    if super::super::hex_encode(&hasher.finalize()) != hash {
        return Err(ReadError::Invalid);
    }
    std::fs::rename(&tmp.0, &dest)?;
    Ok(())
}

/// 置き場へ移す前の一時ファイル。移した後は元の名前が無いので、drop での削除は空振りする。
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// 取り出した DB を検査できなかったときの理由をログに残し、バックアップとして読めないものとして扱う。
/// 手元のディスクの失敗と見分けられないので、調べるときに理由が要る。
fn invalid_db(err: impl std::fmt::Display) -> ReadError {
    tracing::warn!(%err, "failed to check the database in the backup");
    ReadError::Invalid
}

/// 取り出した DB を検査し、今の版の形まで上げる。
async fn prepare_db(db_path: &Path, manifest: &Manifest) -> Result<(), AppError> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .foreign_keys(true);
    // FIX: マイグレーションを1本の接続 (`&mut SqliteConnection`) で走らせると、その future が
    // `Send` と判定されず axum のハンドラに使えない。接続が1つのプールで走らせる。
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(invalid_db)?;
    let result = check_and_migrate(&pool, manifest).await;
    pool.close().await;
    result
}

async fn check_and_migrate(pool: &SqlitePool, manifest: &Manifest) -> Result<(), AppError> {
    // ここで弾くのは、取り出した DB が壊れている・weblav の DB ではない場合。
    // SQLite のエラーの種類で見分けられないため、どれもバックアップとして読めないものとして扱う。
    let check: String = sqlx::query_scalar("PRAGMA quick_check")
        .fetch_one(pool)
        .await
        .map_err(invalid_db)?;
    if check != "ok" {
        return Err(ReadError::Invalid.into());
    }

    let version: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await
        .map_err(invalid_db)?;
    // 目録は書き換えられるので、DB の側でも確かめる。
    if version.is_some_and(|version| version > crate::db::schema_version()) {
        return Err(ReadError::TooNew {
            app_version: manifest.app_version.clone(),
        }
        .into());
    }

    crate::db::MIGRATOR.run(pool).await.map_err(invalid_db)?;

    let violations: Vec<(String,)> = sqlx::query_as("PRAGMA foreign_key_check")
        .fetch_all(pool)
        .await
        .map_err(invalid_db)?;
    if !violations.is_empty() {
        return Err(ReadError::Invalid.into());
    }
    Ok(())
}

/// 今の DB の表の中身を、`db_path` の DB のもので置き換える。セッションは戻さずに全部消し、`revoked` に登録する。
///
/// ADR: DB のファイルを差し替えずに、表の中身を入れ替える。接続のプールは `AppState` を通して
/// 各所に配られていて閉じ直せないため。1つのトランザクションで入れ替えるので、途中で失敗しても今の DB は変わらない。
async fn replace_tables(
    pool: &SqlitePool,
    db_path: &Path,
    revoked: &RevokedSessions,
) -> Result<(), AppError> {
    let db_path = db_path
        .to_str()
        .ok_or_else(|| io::Error::other("the data directory path is not valid UTF-8"))?;
    // ATTACH は接続ごとに効く。付けたままプールへ戻さないよう、切り離した接続を使って閉じる。
    let mut conn = pool.acquire().await?.detach();
    let result = async {
        sqlx::query("ATTACH DATABASE ? AS backup")
            .bind(db_path)
            .execute(&mut conn)
            .await?;
        copy_tables(&mut conn, revoked).await
    }
    .await;
    let _ = conn.close().await;
    Ok(result?)
}

/// 表の名前は実行時に決まるので、この関数の SQL は検査マクロを使えない。
async fn copy_tables(
    conn: &mut SqliteConnection,
    revoked: &RevokedSessions,
) -> Result<(), sqlx::Error> {
    let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
    // 外部キーの検査を確定の時まで待たせる。表を1つずつ入れる途中では、参照先がまだ無いため。
    sqlx::query("PRAGMA defer_foreign_keys = ON")
        .execute(&mut *tx)
        .await?;

    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM main.sqlite_master
         WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'
           AND name NOT IN ('_sqlx_migrations', ?, ?, ?)",
    )
    .bind(SESSION_TABLE)
    .bind(FOLDER_BOOKMARKS_TABLE)
    .bind(LINK_PREVIEWS_TABLE)
    .fetch_all(&mut *tx)
    .await?;

    // 先に全部消してから入れる。入れた後で別の表を消すと、`ON DELETE` の動作が入れた行に及ぶため。
    for table in &tables {
        sqlx::query(&format!("DELETE FROM main.{}", quote(table)))
            .execute(&mut *tx)
            .await?;
    }
    for table in &tables {
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info(?, 'main')")
                .bind(table)
                .fetch_all(&mut *tx)
                .await?;
        let columns = columns
            .iter()
            .map(|c| quote(c))
            .collect::<Vec<_>>()
            .join(", ");
        let table = quote(table);
        sqlx::query(&format!(
            "INSERT INTO main.{table} ({columns}) SELECT {columns} FROM backup.{table}"
        ))
        .execute(&mut *tx)
        .await?;
    }

    // AUTOINCREMENT の続きの番号も戻す。消したユーザーの id を新しいユーザーに使い回さないため。
    sqlx::query("DELETE FROM main.sqlite_sequence")
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO main.sqlite_sequence (name, seq) SELECT name, seq FROM backup.sqlite_sequence",
    )
    .execute(&mut *tx)
    .await?;

    let session_table = quote(SESSION_TABLE);
    let sessions: Vec<String> = sqlx::query_scalar(&format!("SELECT id FROM main.{session_table}"))
        .fetch_all(&mut *tx)
        .await?;
    // 消す前に登録する。消した後で、戻す前から続いていた要求がセッションを書き戻しても残らない
    // (→ `session::Store`)。確定できなければ消えていないので、登録も取り消す。
    revoked.extend(sessions.iter().cloned());
    let deleted = async {
        sqlx::query(&format!("DELETE FROM main.{session_table}"))
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    }
    .await;
    if deleted.is_err() {
        revoked.forget(&sessions);
    }
    deleted?;
    Ok(())
}

/// SQLite の識別子として引用する。
fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// 置き換えた DB から参照されていない blob を消す。
/// 失敗しても戻し自体は済んでいるので、ログに残すだけにする (`contents::gc_blob_if_unreferenced` と同じ)。
async fn remove_unreferenced_blobs(pool: &SqlitePool, blobs_dir: &Path) {
    let referenced: HashSet<String> = match referenced_blobs(pool).await {
        Ok(hashes) => hashes.into_iter().collect(),
        Err(err) => {
            tracing::warn!(%err, "failed to list referenced blobs; unreferenced blobs are left");
            return;
        }
    };
    let blobs_dir = blobs_dir.to_path_buf();
    let result = run_blocking(move || -> io::Result<()> {
        for entry in std::fs::read_dir(&blobs_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !super::super::contents::is_valid_blob_hash(name) || referenced.contains(name) {
                continue;
            }
            if let Err(err) = std::fs::remove_file(entry.path()) {
                tracing::warn!(%err, blob_hash = %name, "failed to remove an unreferenced blob");
            }
        }
        Ok(())
    })
    .await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(err)) => tracing::warn!(%err, "failed to remove unreferenced blobs"),
        Err(err) => tracing::warn!(%err, "failed to remove unreferenced blobs"),
    }
}
