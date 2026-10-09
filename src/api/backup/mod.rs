//! 管理画面からのバックアップとリストア (→ docs/access.md「バックアップとリストア」)。`admin` だけが使える。
//! セットアップの画面から戻す口は `api::setup` にあり、ここの `stage` と `restore_staged` を使う。
//!
//! 戻すのは2段にする。受け取ったファイルをいったん置いて目録を返し、画面で日時と版を
//! 確かめてもらってから、置いたものの id を指して戻す。

mod archive;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Json;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio_util::io::{ReaderStream, SyncIoBridge};
use tower_sessions::Session;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::completed_reader::CompletedReader;
use crate::auth::AdminUser;
use crate::error::{AppError, AppJson, multipart_error_to_app_error, run_blocking};
use crate::state::AppState;

use archive::{Manifest, WorkDir};

pub(crate) use archive::remove_work_root;

/// 受け取ったファイルの、作業用ディレクトリの中での名前。
const UPLOAD_FILE_NAME: &str = "upload.zip";

/// ADR: 受け取ったファイルを戻すまで置いておく時間。確認の画面で迷っても足りる長さにする。
/// 時間では掃除しない。期限を過ぎたものは、次に受け取るか戻すとき、または起動し直したときに消える。
/// 置くのは1つだけなので、残ってもディスクを占めるのは1つ分で済む。
const STAGED_TTL: Duration = Duration::from_secs(30 * 60);

/// zip を作る側と送る側の間に置くバッファの大きさ。
const DOWNLOAD_BUFFER_BYTES: usize = 256 * 1024;

/// 受け取って、戻すのを待っているバックアップ。同時に置くのは最後に受け取った1つだけ。
///
/// `tokio::sync::Mutex` ではなく `std::sync::Mutex` を使う。保持するのは値を差し替える
/// 一瞬だけで、この鍵を持ったまま await しないため (`SetupTokens` と同じ)。
#[derive(Default)]
pub struct RestoreStaging {
    staged: Mutex<Option<Staged>>,
}

struct Staged {
    id: String,
    dir: WorkDir,
    manifest: Manifest,
    expires_at: Instant,
}

impl RestoreStaging {
    fn put(&self, id: String, dir: WorkDir, manifest: Manifest) {
        let previous = self.lock().replace(Staged {
            id,
            dir,
            manifest,
            expires_at: Instant::now() + STAGED_TTL,
        });
        // 前のファイルは鍵を放してから消す。
        drop(previous);
    }

    /// 戻すのに失敗したものを置き直す。期限は受け取ったときのまま。
    /// 間に新しく受け取っていれば、そちらを残す。
    fn put_back(&self, staged: Staged) {
        let mut slot = self.lock();
        let rejected = if slot.is_none() {
            slot.replace(staged)
        } else {
            Some(staged)
        };
        drop(slot);
        drop(rejected);
    }

    /// `id` のものが期限内に置かれていれば取り出す。期限切れのものは、ここで捨てる。
    fn take(&self, id: &str) -> Option<Staged> {
        let mut staged = self.lock();
        let matches = staged
            .as_ref()
            .is_some_and(|s| s.expires_at > Instant::now() && s.id == id);
        let expired = staged
            .as_ref()
            .is_some_and(|s| s.expires_at <= Instant::now());
        let taken = if matches || expired {
            staged.take()
        } else {
            None
        };
        drop(staged);
        taken.filter(|_| matches)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Staged>> {
        self.staged.lock().expect("restore staging lock poisoned")
    }
}

/// 受け取ったバックアップの目録。確認の画面に出す。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct StagedBackupResponse {
    /// 戻すときに指す id。
    id: String,
    /// 作った日時 (UNIX 秒)。
    created_at: i64,
    /// 作った weblav の版。
    app_version: String,
}

/// utoipa の OpenAPI ドキュメント用のダミー構造体 (実際は `axum::extract::Multipart` で受け取る)。
#[derive(Debug, Deserialize, ToSchema)]
#[allow(dead_code)]
pub(super) struct StageBackupRequest {
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct RestoreBackupRequest {
    /// 受け取ったときに返した id。
    id: String,
}

/// 今の DB とアップロードしたファイルを1つの zip にして返す。作りながら送る。
#[utoipa::path(
    get,
    path = "/admin/backup",
    responses(
        (status = OK, content_type = "application/zip", description = "バックアップの zip"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
    )
)]
async fn download_backup(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let snapshot = {
        let _guard = state.contents_write_lock.lock().await;
        archive::snapshot(&state.pool, &state.blobs_dir, &state.backup_work_dir).await?
    };
    let manifest = Manifest::now();

    let (reader, writer) = tokio::io::duplex(DOWNLOAD_BUFFER_BYTES);
    let writer = SyncIoBridge::new(writer);
    let finished = Arc::new(AtomicBool::new(false));
    let finished_by_writer = Arc::clone(&finished);
    tokio::task::spawn_blocking(move || {
        match archive::write_zip(&snapshot, &manifest, writer) {
            // 終わった印を付けてから閉じる。読む側は閉じたのを見てから印を見る。
            Ok(writer) => {
                finished_by_writer.store(true, Ordering::Release);
                drop(writer);
            }
            // 受け取る側が途中で切った場合もここに来る。
            Err(err) => tracing::warn!(%err, "failed to write the backup"),
        }
    });

    Ok((
        [
            (header::CONTENT_TYPE, "application/zip"),
            // ファイル名は画面が付ける (`download` 属性)。作った日時を利用者の時刻で入れるため。
            (header::CONTENT_DISPOSITION, "attachment"),
        ],
        Body::from_stream(ReaderStream::new(CompletedReader::new(
            reader,
            finished,
            "the backup was not completed",
        ))),
    ))
}

/// 戻すバックアップを受け取り、目録を返す。まだ何も置き換えない。
#[utoipa::path(
    post,
    path = "/admin/backup/restore/stage",
    request_body(content = StageBackupRequest, content_type = "multipart/form-data"),
    responses(
        (status = OK, body = StagedBackupResponse, description = "受け取ったバックアップの目録"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 422, body = crate::error::ErrorResponse, description = "バックアップではない・新しい版で作られた"),
    )
)]
async fn stage_restore(
    _admin: AdminUser,
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<Json<StagedBackupResponse>, AppError> {
    stage(&state, multipart).await.map(Json)
}

/// 受け取ったバックアップで、今のデータを置き換える。全員のセッションを消すので、この要求の送り手もログインし直す。
#[utoipa::path(
    post,
    path = "/admin/backup/restore",
    request_body = RestoreBackupRequest,
    responses(
        (status = 204, description = "戻した"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 404, body = crate::error::ErrorResponse, description = "受け取ったものが無い・期限切れ"),
        (status = 409, body = crate::error::ErrorResponse, description = "アーカイブの走査中"),
        (status = 422, body = crate::error::ErrorResponse, description = "バックアップが壊れている"),
    )
)]
async fn restore(
    admin: AdminUser,
    session: Session,
    State(state): State<AppState>,
    AppJson(payload): AppJson<RestoreBackupRequest>,
) -> Result<StatusCode, AppError> {
    {
        let _users_guard = state.users_write_lock.lock().await;
        restore_staged(&state, &payload.id).await?;
    }
    tracing::info!(user_id = admin.id, "restored data from a backup");
    end_session(&session).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// multipart の `file` を受け取って置き、目録を返す。
pub(super) async fn stage(
    state: &AppState,
    mut multipart: Multipart,
) -> Result<StagedBackupResponse, AppError> {
    let root = state.backup_work_dir.clone();
    let dir = run_blocking(move || WorkDir::create(&root)).await??;
    let path = dir.path().join(UPLOAD_FILE_NAME);

    let mut received = false;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(multipart_error_to_app_error)?
    {
        if field.name() != Some("file") {
            continue;
        }
        if received {
            return Err(AppError::Validation("file is given twice".to_string()));
        }
        let mut file = tokio::fs::File::create_new(&path).await?;
        while let Some(chunk) = field.chunk().await.map_err(multipart_error_to_app_error)? {
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        received = true;
    }
    if !received {
        return Err(AppError::Validation("file is required".to_string()));
    }

    let manifest = run_blocking(move || archive::read_manifest(&path)).await??;
    let id = super::random_hex::<16>()?;
    let response = StagedBackupResponse {
        id: id.clone(),
        created_at: manifest.created_at,
        app_version: manifest.app_version.clone(),
    };
    state.restore_staging.put(id, dir, manifest);
    Ok(response)
}

/// 置いておいたバックアップ `id` で、今のデータを置き換える。
///
/// 呼び出し側は `users_write_lock` を持っていること (鍵の順は `users_write_lock` が先、
/// → `AppState::users_write_lock`)。セットアップの口は、「admin が0人」の確認から
/// 戻し終えるまでを同じ鍵で直列化するため、鍵を外で取る。
pub(super) async fn restore_staged(state: &AppState, id: &str) -> Result<(), AppError> {
    let _contents_guard = state.contents_write_lock.lock().await;
    // 走査は `contents_write_lock` を取らずに結果を書き込むので、走っている間は戻さない。
    if !state.archive_scans.is_idle() {
        return Err(AppError::Conflict(
            "an archive is being scanned".to_string(),
        ));
    }
    let staged = state.restore_staging.take(id).ok_or(AppError::NotFound)?;
    let result = archive::restore(
        &state.pool,
        &state.blobs_dir,
        &state.backup_work_dir,
        &staged.dir.path().join(UPLOAD_FILE_NAME),
        &staged.manifest,
        &state.revoked_sessions,
    )
    .await;
    match result {
        // 戻した DB の公開フォルダーを読めるようにする (→ docs/distribution.md「ビルド・配布の方法」)。
        Ok(()) => {
            crate::folder_access::restore(&state.pool).await;
            Ok(())
        }
        // ディスクの空き不足のような一時的な失敗で、大きなファイルを送り直させないため。
        Err(err) => {
            state.restore_staging.put_back(staged);
            Err(err)
        }
    }
}

/// 戻した後で、この要求のセッションを消す。
/// 戻した DB では、このセッションの利用者 id が別の人を指しうるため。
pub(super) async fn end_session(session: &Session) -> Result<(), AppError> {
    session
        .flush()
        .await
        .map_err(|err| AppError::Internal(crate::auth::Error::Session(err)))
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    // ADR: 受け取る大きさに上限を設けない。DB とアップロードしたファイルの全部が入るので、
    // 決まった上限では足りなくなりうる。使えるのは admin と、この PC の中からのセットアップだけ。
    let upload_routes = OpenApiRouter::new()
        .routes(routes!(stage_restore))
        .layer(DefaultBodyLimit::disable());

    OpenApiRouter::new()
        .routes(routes!(download_backup))
        .routes(routes!(restore))
        .merge(upload_routes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn staged_dir(tmp: &TempDir) -> WorkDir {
        WorkDir::create(tmp.path()).expect("作業用のディレクトリを作れなかった")
    }

    fn manifest() -> Manifest {
        Manifest::now()
    }

    #[test]
    fn staged_backup_is_taken_only_by_its_own_id() {
        let tmp = TempDir::new("staged_backup_is_taken_only_by_its_own_id");
        let staging = RestoreStaging::default();
        staging.put("a".to_string(), staged_dir(&tmp), manifest());

        assert!(staging.take("b").is_none());
        assert!(staging.take("a").is_some());
        assert!(staging.take("a").is_none(), "取り出したものは残らない");
    }

    /// 一時的な失敗で戻せなかったものは、送り直さずにもう一度戻せる。
    #[test]
    fn put_back_backup_can_be_taken_again() {
        let tmp = TempDir::new("put_back_backup_can_be_taken_again");
        let staging = RestoreStaging::default();
        staging.put("a".to_string(), staged_dir(&tmp), manifest());
        let staged = staging.take("a").expect("取り出せなかった");

        staging.put_back(staged);

        assert!(staging.take("a").is_some());
    }

    /// 戻している間に新しく受け取ったものがあれば、そちらを残す。
    #[test]
    fn put_back_keeps_a_newer_backup() {
        let tmp = TempDir::new("put_back_keeps_a_newer_backup");
        let staging = RestoreStaging::default();
        staging.put("a".to_string(), staged_dir(&tmp), manifest());
        let staged = staging.take("a").expect("取り出せなかった");
        let old_path = staged.dir.path().to_path_buf();
        staging.put("b".to_string(), staged_dir(&tmp), manifest());

        staging.put_back(staged);

        assert!(staging.take("a").is_none());
        assert!(!old_path.exists(), "置き直せなかったものは消す");
        assert!(staging.take("b").is_some());
    }

    #[test]
    fn staging_a_new_backup_removes_the_previous_file() {
        let tmp = TempDir::new("staging_a_new_backup_removes_the_previous_file");
        let staging = RestoreStaging::default();
        let first = staged_dir(&tmp);
        let first_path = first.path().to_path_buf();
        staging.put("a".to_string(), first, manifest());

        staging.put("b".to_string(), staged_dir(&tmp), manifest());

        assert!(!first_path.exists());
        assert!(staging.take("a").is_none());
    }

    #[test]
    fn expired_backup_is_not_taken_and_its_file_is_removed() {
        let tmp = TempDir::new("expired_backup_is_not_taken_and_its_file_is_removed");
        let staging = RestoreStaging::default();
        let dir = staged_dir(&tmp);
        let path = dir.path().to_path_buf();
        staging.put("a".to_string(), dir, manifest());
        staging
            .lock()
            .as_mut()
            .expect("置いたものが無い")
            .expires_at = Instant::now();

        assert!(staging.take("b").is_none());
        assert!(
            !path.exists(),
            "期限切れのものは、違う id で問い合わせても捨てる"
        );
    }
}
