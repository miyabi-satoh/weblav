//! 共有の中のコピー・移動・削除の口と、その進み具合の口。
//!
//! どれも裏で続く処理 (→ `jobs`) として走らせ、すぐに 202 で返す。始める前の確かめ (`.../check`) で、
//! 置き先の同じ名前・大きさ・空き、消すと開けなくなるコンテンツを画面に伝えられる。
//!
//! できることは、コピーが元で「見る」以上、移動・削除が元で「すべて」、置き先で「追加できる」以上。
//! 置き先の同じ名前を上書きするには、置き先でも「すべて」が要る (「追加できる」は、今あるものを消せないため)。

use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::api::contents::Visibility;
use crate::api::error_detail::ValidationDetail;
use crate::auth::AuthUser;
use crate::error::{AppError, AppJson, run_blocking};
use crate::state::AppState;

use super::browse::ListedAs;
use super::fsops::{self, TrashPolicy};
use super::jobs::{Job, JobOp, ShareJobResponse};
use super::log::{self, LoggedPlace, OpRecord, OpResult, ShareOp, record_or_warn};
use super::paths::{Resolved, ensure_not_protected, ensure_without_own_dirs, resolve_existing};
use super::repoint::{self, Change};
use super::{ShareAccess, require};
use crate::api::fs::join_rel;

/// コピーか移動か。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum TransferOp {
    Copy,
    Move,
}

/// 置き先に同じ名前があるときの扱い。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum OnConflict {
    /// 新しいものを用意できてから、今あるものを消して (ごみ箱があればごみ箱へ) 置く。置き先で「すべて」が要る。
    Overwrite,
    /// 番号を付けた名前で置く。
    Rename,
    /// 置かない。
    Skip,
}

/// 動かす・写すもの。どれも同じ共有の中。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferFrom {
    share_id: i64,
    /// 共有からの相対パス。
    paths: Vec<String>,
}

/// 置き先のフォルダー。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferTo {
    share_id: i64,
    /// 共有からの相対パス。空文字列は共有そのもの。
    #[serde(default)]
    path: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferCheckRequest {
    op: TransferOp,
    from: TransferFrom,
    to: TransferTo,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferRequest {
    op: TransferOp,
    from: TransferFrom,
    to: TransferTo,
    on_conflict: OnConflict,
}

/// 置き先に既にある同じ名前のもの。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferConflict {
    name: String,
    /// 置き先にあるものがフォルダーか。
    is_dir: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferCheckResponse {
    /// 置き先に同じ名前があるもの。同じフォルダーの中へのコピー (番号を付けて写す) と、
    /// 同じフォルダーの中への移動 (何もしない) は含めない。
    conflicts: Vec<TransferConflict>,
    /// 動かす・写すファイルの大きさの合計。
    total_bytes: u64,
    /// 置き先のディスクの空き。
    available_bytes: u64,
    /// 上書きで消える置き先のものを、OS のごみ箱へ送れるか。偽なら、上書きすると元に戻せない。
    trash: bool,
    /// 上書きすると、中身が変わるか開けなくなるコンテンツ (フォルダーの登録先かショートカットの先が、`conflicts` のものかその中)。
    /// 見ている人の管理画面の一覧に出ないもの (他人の「作成者のみ」) は含めない。
    listed: Vec<ListedAs>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobStartedResponse {
    /// 進み具合を `GET /shares/jobs/{id}` で読む。
    job_id: String,
}

/// 動かす・写す1つ。
struct Source {
    resolved: Resolved,
    name: String,
    is_dir: bool,
    bytes: u64,
    items: u64,
}

struct PreparedTransfer {
    sources: Vec<Source>,
    dest: Resolved,
}

impl PreparedTransfer {
    fn total(&self) -> (u64, u64) {
        self.sources.iter().fold((0, 0), |(items, bytes), source| {
            (items + source.items, bytes + source.bytes)
        })
    }
}

/// 選んだものと置き先を解決し、できることを確かめる。
async fn prepare_transfer(
    state: &AppState,
    user: &AuthUser,
    op: TransferOp,
    from: &TransferFrom,
    to: &TransferTo,
) -> Result<PreparedTransfer, AppError> {
    if from.paths.is_empty() {
        return Err(AppError::Validation("no entry was chosen".to_string()));
    }
    let dest = resolve_existing(state, user, to.share_id, &to.path).await?;
    require(&dest, ShareAccess::Add)?;
    let mut resolved = Vec::new();
    for rel in &from.paths {
        let source = resolve_existing(state, user, from.share_id, rel).await?;
        match op {
            TransferOp::Copy => {
                require(&source, ShareAccess::Read)?;
                ensure_without_own_dirs(state, &source.canonical).await?;
            }
            TransferOp::Move => {
                require(&source, ShareAccess::All)?;
                ensure_not_protected(state, &source).await?;
            }
        }
        // 同じものを2回選んでいれば1つにする。
        if !resolved
            .iter()
            .any(|other: &Resolved| other.canonical == source.canonical)
        {
            resolved.push(source);
        }
    }
    let dest_dir = dest.canonical.clone();
    let sources = run_blocking(move || {
        if !dest_dir.is_dir() {
            return Err(AppError::NotFound);
        }
        resolved
            .into_iter()
            .map(|resolved| {
                fsops::ensure_not_into_itself(&resolved.canonical, &dest_dir)?;
                let (bytes, items) =
                    fsops::tree_size(&resolved.canonical).map_err(fsops::map_io_error)?;
                Ok(Source {
                    name: fsops::file_name(&resolved.canonical),
                    is_dir: std::fs::symlink_metadata(&resolved.canonical)
                        .map_err(fsops::map_io_error)?
                        .is_dir(),
                    resolved,
                    bytes,
                    items,
                })
            })
            .collect::<Result<Vec<_>, AppError>>()
    })
    .await??;
    Ok(PreparedTransfer { sources, dest })
}

/// 置き先にある、`source` と同じ名前の別のもの。
fn conflict_of(source: &Source, dest_dir: &FsPath) -> Option<TransferConflict> {
    let target = dest_dir.join(&source.name);
    if !fsops::exists(&target) || fsops::same_entry(&source.resolved.canonical, &target) {
        return None;
    }
    Some(TransferConflict {
        name: source.name.clone(),
        is_dir: std::fs::symlink_metadata(&target).is_ok_and(|metadata| metadata.is_dir()),
    })
}

/// コピー・移動を始める前の確かめ。置き先の同じ名前・大きさ・空きと、上書きで響くコンテンツを返す。何も変えない。
#[utoipa::path(
    post,
    path = "/shares/transfer/check",
    request_body = TransferCheckRequest,
    responses(
        (status = OK, body = TransferCheckResponse, description = "始める前の確かめ"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "元か置き先で、この操作ができない"),
        (status = 404, body = crate::error::ErrorResponse, description = "共有・ファイル・置き先が無い、見られない"),
        (status = 422, body = crate::error::ErrorResponse, description = "フォルダーをそれ自身の中へ (`transferIntoItself`)・共有そのもの (`entryProtected`)"),
    )
)]
async fn check_transfer(
    user: AuthUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<TransferCheckRequest>,
) -> Result<Json<TransferCheckResponse>, AppError> {
    let prepared = prepare_transfer(&state, &user, payload.op, &payload.from, &payload.to).await?;
    let (_, total_bytes) = prepared.total();
    let share_trash = state.share_trash.clone();
    let dest_dir = prepared.dest.canonical.clone();
    let (conflicts, available_bytes, trash) = run_blocking(move || {
        let dest_dir = &prepared.dest.canonical;
        let conflicts: Vec<TransferConflict> = prepared
            .sources
            .iter()
            .filter_map(|source| conflict_of(source, dest_dir))
            .collect();
        let available_bytes = fs4::available_space(dest_dir).map_err(fsops::map_io_error)?;
        Ok::<_, AppError>((conflicts, available_bytes, share_trash.available(dest_dir)))
    })
    .await??;
    let doomed: Vec<PathBuf> = conflicts
        .iter()
        .map(|conflict| dest_dir.join(&conflict.name))
        .collect();
    let listed = listed_under(&state, &user, &doomed).await?;
    Ok(Json(TransferCheckResponse {
        conflicts,
        total_bytes,
        available_bytes,
        trash,
        listed,
    }))
}

/// コピー・移動を始める。進み具合は `GET /shares/jobs/{id}` で読む。
///
/// 置き先の同じ名前は `onConflict` で扱う。上書きは、新しいものを用意できてから、今あるものをごみ箱へ送って置く
/// (ごみ箱を使えない場所では完全に消す)。同じフォルダーの中へのコピーは、番号を付けた名前で写す。
#[utoipa::path(
    post,
    path = "/shares/transfer",
    request_body = TransferRequest,
    responses(
        (status = ACCEPTED, body = JobStartedResponse, description = "始めた"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "元か置き先で、この操作ができない (上書きには置き先で「すべて」が要る)"),
        (status = 404, body = crate::error::ErrorResponse, description = "共有・ファイル・置き先が無い、見られない"),
        (status = 422, body = crate::error::ErrorResponse, description = "フォルダーをそれ自身の中へ (`transferIntoItself`)・共有そのものや共有を含むもの (`entryProtected`)"),
        (status = 507, body = crate::error::ErrorResponse, description = "置き先の空きが足りない (コピー)"),
    )
)]
async fn start_transfer(
    user: AuthUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<TransferRequest>,
) -> Result<(StatusCode, Json<JobStartedResponse>), AppError> {
    let prepared = prepare_transfer(&state, &user, payload.op, &payload.from, &payload.to).await?;
    if payload.on_conflict == OnConflict::Overwrite {
        ensure_overwritable(&state, &user, &prepared, payload.to.share_id).await?;
    }
    let (total_items, total_bytes) = prepared.total();
    if payload.op == TransferOp::Copy {
        let dest_dir = prepared.dest.canonical.clone();
        run_blocking(move || fsops::ensure_space(&dest_dir, total_bytes)).await??;
    }
    let (job_op, share_op) = match payload.op {
        TransferOp::Copy => (JobOp::Copy, ShareOp::Copy),
        TransferOp::Move => (JobOp::Move, ShareOp::Move),
    };
    let job = state
        .share_jobs
        .start(user.id, job_op, total_items, total_bytes)?;
    let first = &prepared.sources[0].resolved;
    let log_id = record_or_warn(
        &state.pool,
        &OpRecord::new(
            share_op,
            &user,
            first.root_id,
            &first.root_name,
            prepared
                .sources
                .iter()
                .map(|source| source.resolved.rel.clone())
                .collect(),
        )
        .dest(LoggedPlace {
            root_id: prepared.dest.root_id,
            root_name: prepared.dest.root_name.clone(),
            path: prepared.dest.rel.clone(),
        })
        .result(OpResult::Running),
    )
    .await;
    let job_id = job.id.clone();
    tokio::spawn(run_transfer(
        state,
        job,
        log_id,
        prepared,
        payload.op,
        payload.on_conflict,
        user,
    ));
    Ok((StatusCode::ACCEPTED, Json(JobStartedResponse { job_id })))
}

/// 上書きしてよいか。置き先で「すべて」ができ、上書きされるものが共有でも共有を含むものでもなく、
/// 動かす・写すものを含んでいない (含んでいれば、消すと元も消える) こと。
async fn ensure_overwritable(
    state: &AppState,
    user: &AuthUser,
    prepared: &PreparedTransfer,
    dest_share_id: i64,
) -> Result<(), AppError> {
    require(&prepared.dest, ShareAccess::All)?;
    for source in &prepared.sources {
        let target = prepared.dest.canonical.join(&source.name);
        let source_path = source.resolved.canonical.clone();
        let check = target.clone();
        let taken =
            run_blocking(move || fsops::exists(&check) && !fsops::same_entry(&source_path, &check))
                .await?;
        if !taken {
            continue;
        }
        let target = resolve_existing(
            state,
            user,
            dest_share_id,
            &join_rel(&prepared.dest.rel, &source.name),
        )
        .await?;
        ensure_not_protected(state, &target).await?;
        if source.resolved.canonical.starts_with(&target.canonical) {
            return Err(AppError::ValidationDetailed {
                message: "cannot overwrite a folder that holds what is being moved or copied"
                    .to_string(),
                detail: ValidationDetail::TransferIntoItself,
            });
        }
    }
    Ok(())
}

/// 実際に済んだこと。進み具合の表示 (飛ばしたものも済んだものに数えて全体まで進める) とは別に持つ。
#[derive(Default)]
struct Applied {
    changes: Vec<Change>,
    /// 置いたものの数と大きさ。操作の記録に書く。
    items: u64,
    bytes: u64,
}

/// コピー・移動を1つずつ進め、終わったら DB に映して記録を閉じる。
async fn run_transfer(
    state: AppState,
    job: Arc<Job>,
    log_id: Option<i64>,
    prepared: PreparedTransfer,
    op: TransferOp,
    on_conflict: OnConflict,
    user: AuthUser,
) {
    let mut applied = Applied::default();
    let result = transfer_all(
        &state,
        &job,
        &prepared,
        op,
        on_conflict,
        &user,
        &mut applied,
    )
    .await;
    // 移動は、入れた先の一覧の表示の読み直しまで `repoint` が受け持つ。
    repoint::apply(&state, &applied.changes).await;
    if op == TransferOp::Copy {
        refresh_list_views(&state, &prepared.dest.canonical).await;
    }
    finish(
        &state,
        &job,
        log_id,
        &result,
        (applied.items, applied.bytes),
    )
    .await;
}

async fn transfer_all(
    state: &AppState,
    job: &Arc<Job>,
    prepared: &PreparedTransfer,
    op: TransferOp,
    on_conflict: OnConflict,
    user: &AuthUser,
    applied: &mut Applied,
) -> Result<(), AppError> {
    let mut progress_items = 0;
    let mut progress_bytes = 0;
    for source in &prepared.sources {
        if job.progress.is_cancelled() {
            break;
        }
        let step = Step {
            src: source.resolved.canonical.clone(),
            is_dir: source.is_dir,
            dest_dir: prepared.dest.canonical.clone(),
            name: source.name.clone(),
            op,
            on_conflict,
        };
        let job_in = Arc::clone(job);
        let trash = state.share_trash.clone();
        let (step_changes, result) = run_blocking(move || {
            let mut changes = Vec::new();
            let result = step.run(&job_in, &trash, &mut changes);
            (changes, result)
        })
        .await?;
        // 置くのに失敗しても、消してしまったものは記録に残す。
        if step_changes
            .iter()
            .any(|change| matches!(change, Change::Overwritten(_) | Change::Deleted(_)))
        {
            log_overwritten(state, user, &prepared.dest, &source.name).await;
        }
        applied.changes.extend(step_changes);
        if result? {
            applied.items += source.items;
            applied.bytes += source.bytes;
        }
        // 移動 (名前の付け替え) と飛ばしたものは中を数えないので、済んだ分を選んだものごとにそろえる。
        progress_items += source.items;
        progress_bytes += source.bytes;
        if !job.progress.is_cancelled() {
            job.progress
                .items
                .fetch_max(progress_items, Ordering::Relaxed);
            job.progress
                .bytes
                .fetch_max(progress_bytes, Ordering::Relaxed);
        }
    }
    Ok(())
}

/// 上書きで消したものを、削除の1行として残す。誰が何を消したかを記録から辿れるようにするため。
async fn log_overwritten(state: &AppState, user: &AuthUser, dest: &Resolved, name: &str) {
    record_or_warn(
        &state.pool,
        &OpRecord::new(
            ShareOp::Delete,
            user,
            dest.root_id,
            &dest.root_name,
            vec![join_rel(&dest.rel, name)],
        )
        .counts(1, 0),
    )
    .await;
}

/// 1つ分のコピー・移動。
struct Step {
    src: PathBuf,
    is_dir: bool,
    dest_dir: PathBuf,
    name: String,
    op: TransferOp,
    on_conflict: OnConflict,
}

impl Step {
    /// ブロッキングI/Oを行うため、`spawn_blocking` の中で呼ぶこと。DB に映す変更を `changes` に足す
    /// (途中で失敗しても、それまでにした変更は映す)。実際に置いたら真を返す
    /// (飛ばした・同じフォルダーの中への移動・中止は偽)。
    fn run(
        self,
        job: &Job,
        trash: &TrashPolicy,
        changes: &mut Vec<Change>,
    ) -> Result<bool, AppError> {
        let mut name = self.name.clone();
        let target = self.dest_dir.join(&self.name);
        if fsops::exists(&target) {
            if fsops::same_entry(&self.src, &target) {
                match self.op {
                    // 同じフォルダーの中への移動は、何もしない。
                    TransferOp::Move => return Ok(false),
                    TransferOp::Copy => {
                        name = fsops::free_name(&self.dest_dir, &self.name, self.is_dir);
                    }
                }
            } else {
                match self.on_conflict {
                    OnConflict::Skip => {
                        job.skip();
                        return Ok(false);
                    }
                    OnConflict::Rename => {
                        name = fsops::free_name(&self.dest_dir, &self.name, self.is_dir);
                    }
                    OnConflict::Overwrite => {
                        return self.overwrite(target, job, trash, changes);
                    }
                }
            }
        }
        match self.op {
            TransferOp::Copy => {
                let copied = fsops::copy_tree(&self.src, &self.dest_dir, &name, &job.progress)?;
                Ok(copied.is_some())
            }
            TransferOp::Move => {
                let moved = fsops::move_entry(&self.src, &self.dest_dir, &name, &job.progress)?;
                let placed = moved.is_some();
                if let Some(to) = moved {
                    changes.push(Change::Moved { from: self.src, to });
                }
                Ok(placed)
            }
        }
    }

    /// 今ある `target` と置き換える。消すのは新しいものを用意できてからなので、中止しても、用意するのに
    /// 失敗しても `target` は残る (→ `fsops::copy_over`・`fsops::move_over`)。
    fn overwrite(
        self,
        target: PathBuf,
        job: &Job,
        trash: &TrashPolicy,
        changes: &mut Vec<Change>,
    ) -> Result<bool, AppError> {
        let mut outcome = fsops::OverwriteOutcome::default();
        let result = match self.op {
            TransferOp::Copy => {
                fsops::copy_over(&self.src, &target, &job.progress, trash, &mut outcome)
            }
            TransferOp::Move => {
                fsops::move_over(&self.src, &target, &job.progress, trash, &mut outcome)
            }
        };
        if outcome.placed {
            changes.push(Change::Overwritten(target.clone()));
        } else if outcome.target_removed {
            // 置けなかったときは、ふつうの削除と同じ。
            changes.push(Change::Deleted(target.clone()));
        }
        if self.op == TransferOp::Move && matches!(result, Ok(true)) {
            changes.push(Change::Moved {
                from: self.src,
                to: target,
            });
        }
        result
    }
}

/// `dir` に足したものを、一覧の表示を使うフォルダーの索引に入れさせる。
async fn refresh_list_views(state: &AppState, dir: &FsPath) {
    match repoint::list_views_containing(&state.pool, dir).await {
        Ok(ids) => repoint::refresh_after_change(state, ids),
        Err(err) => {
            tracing::warn!(error = %crate::error::error_chain(&err), "finding folders to refresh after a file operation failed");
        }
    }
}

/// 処理と記録を閉じる。`done` は、記録に書く実際に済んだものの数と大きさ。
async fn finish(
    state: &AppState,
    job: &Job,
    log_id: Option<i64>,
    result: &Result<(), AppError>,
    done: (u64, u64),
) {
    if let Err(err) = result {
        tracing::info!(job = %job.id, error = %crate::error::error_chain(err), "a share job failed");
    }
    // 記録を閉じてから処理を終わったことにする。終わったのを見た画面やテストが記録を読んだとき、
    // まだ途中のままになっていないように。
    if let Some(log_id) = log_id {
        close_log(state, job, log_id, result, done).await;
    }
    job.finish(result);
}

async fn close_log(
    state: &AppState,
    job: &Job,
    log_id: i64,
    result: &Result<(), AppError>,
    (items, bytes): (u64, u64),
) {
    let (state_code, error) = match result {
        Ok(()) if job.progress.is_cancelled() => (OpResult::Cancelled, None),
        Ok(()) => (OpResult::Succeeded, None),
        Err(err) => (OpResult::Failed, Some(err.code().to_string())),
    };
    log::finish(
        &state.pool,
        log_id,
        state_code,
        items,
        bytes,
        error.as_deref(),
    )
    .await;
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteCheckRequest {
    /// 消すものの、共有からの相対パス。
    paths: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRequest {
    paths: Vec<String>,
    /// 完全に消す。偽なら、ごみ箱へ送る (使えない場所では 409 `trash_unavailable`)。
    #[serde(default)]
    permanent: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteCheckResponse {
    /// 消すものをどれもごみ箱へ送れるか。偽なら、消すと元に戻せない。
    trash: bool,
    /// 消すと開けなくなるコンテンツ (フォルダーの登録先かショートカットの先が、消すものかその中)。
    /// 見ている人の管理画面の一覧に出ないもの (他人の「作成者のみ」) は含めない。
    listed: Vec<ListedAs>,
}

/// 登録先か先が `doomed` (実体のパス) のどれかかその中である、フォルダーのコンテンツとショートカット。
/// `user` の管理画面の一覧に出ないもの (他人の「作成者のみ」) は含めない。「非表示」は、戻せば開くものなので含める。
async fn listed_under(
    state: &AppState,
    user: &AuthUser,
    doomed: &[PathBuf],
) -> Result<Vec<ListedAs>, AppError> {
    let doomed = doomed
        .iter()
        .map(|path| crate::api::fs::stored_path(path))
        .collect::<Result<Vec<_>, _>>()?;
    let rows = sqlx::query!(
        r#"SELECT id as "id!", title, path as "path!", visibility as "visibility: Visibility", created_by
           FROM contents
           WHERE type IN ('shortcut', 'folder') AND path IS NOT NULL
           ORDER BY id"#
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|row| {
            crate::api::contents::can_see_in_admin_list(user, row.visibility, row.created_by)
                && doomed
                    .iter()
                    .any(|path| FsPath::new(&row.path).starts_with(path))
        })
        .map(|row| ListedAs {
            content_id: row.id,
            title: row.title,
        })
        .collect())
}

/// 消すものを解決し、できることを確かめる。
async fn prepare_delete(
    state: &AppState,
    user: &AuthUser,
    id: i64,
    paths: &[String],
) -> Result<Vec<Resolved>, AppError> {
    if paths.is_empty() {
        return Err(AppError::Validation("no entry was chosen".to_string()));
    }
    let mut resolved: Vec<Resolved> = Vec::new();
    for rel in paths {
        let entry = resolve_existing(state, user, id, rel).await?;
        require(&entry, ShareAccess::All)?;
        ensure_not_protected(state, &entry).await?;
        if !resolved
            .iter()
            .any(|other| other.canonical == entry.canonical)
        {
            resolved.push(entry);
        }
    }
    Ok(resolved)
}

/// 消すものがどれもごみ箱へ送れるか。
async fn all_trashable(state: &AppState, entries: &[Resolved]) -> Result<bool, AppError> {
    let trash = state.share_trash.clone();
    let paths: Vec<PathBuf> = entries
        .iter()
        .map(|entry| entry.canonical.clone())
        .collect();
    run_blocking(move || paths.iter().all(|path| trash.available(path))).await
}

/// 消す前の確かめ。ごみ箱へ送れるかと、消すと開けなくなるコンテンツを返す。何も変えない。
#[utoipa::path(
    post,
    path = "/shares/{id}/delete/check",
    params(("id" = i64, Path)),
    request_body = DeleteCheckRequest,
    responses(
        (status = OK, body = DeleteCheckResponse, description = "消す前の確かめ"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "この場所で消せない"),
        (status = 404, body = crate::error::ErrorResponse, description = "共有・ファイルが無い、見られない"),
        (status = 422, body = crate::error::ErrorResponse, description = "共有そのものや共有を含むもの (`entryProtected`)"),
    )
)]
async fn check_delete(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<DeleteCheckRequest>,
) -> Result<Json<DeleteCheckResponse>, AppError> {
    let entries = prepare_delete(&state, &user, id, &payload.paths).await?;
    let trash = all_trashable(&state, &entries).await?;
    let doomed: Vec<PathBuf> = entries
        .iter()
        .map(|entry| entry.canonical.clone())
        .collect();
    let listed = listed_under(&state, &user, &doomed).await?;
    Ok(Json(DeleteCheckResponse { trash, listed }))
}

/// 消し始める。進み具合は `GET /shares/jobs/{id}` で読む。
#[utoipa::path(
    post,
    path = "/shares/{id}/delete",
    params(("id" = i64, Path)),
    request_body = DeleteRequest,
    responses(
        (status = ACCEPTED, body = JobStartedResponse, description = "始めた"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "この場所で消せない"),
        (status = 404, body = crate::error::ErrorResponse, description = "共有・ファイルが無い、見られない"),
        (status = 409, body = crate::error::ErrorResponse, description = "ごみ箱を使えない場所 (`trash_unavailable`)"),
        (status = 422, body = crate::error::ErrorResponse, description = "共有そのものや共有を含むもの (`entryProtected`)"),
    )
)]
async fn start_delete(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<DeleteRequest>,
) -> Result<(StatusCode, Json<JobStartedResponse>), AppError> {
    let entries = prepare_delete(&state, &user, id, &payload.paths).await?;
    if !payload.permanent && !all_trashable(&state, &entries).await? {
        return Err(AppError::TrashUnavailable);
    }
    let job = state.share_jobs.start(
        user.id,
        JobOp::Delete,
        u64::try_from(entries.len()).unwrap_or(u64::MAX),
        0,
    )?;
    let log_id = record_or_warn(
        &state.pool,
        &OpRecord::new(
            ShareOp::Delete,
            &user,
            entries[0].root_id,
            &entries[0].root_name,
            entries.iter().map(|entry| entry.rel.clone()).collect(),
        )
        .result(OpResult::Running),
    )
    .await;
    let job_id = job.id.clone();
    let paths = entries.into_iter().map(|entry| entry.canonical).collect();
    tokio::spawn(run_delete(state, job, log_id, paths, payload.permanent));
    Ok((StatusCode::ACCEPTED, Json(JobStartedResponse { job_id })))
}

async fn run_delete(
    state: AppState,
    job: Arc<Job>,
    log_id: Option<i64>,
    paths: Vec<PathBuf>,
    permanent: bool,
) {
    let mut changes = Vec::new();
    let mut result = Ok(());
    for path in paths {
        if job.progress.is_cancelled() {
            break;
        }
        let trash = state.share_trash.clone();
        let target = path.clone();
        let deleted = run_blocking(move || fsops::delete(&[target], permanent, &trash)).await;
        match deleted {
            Ok(Ok(())) => {
                changes.push(Change::Deleted(path));
                job.progress.add_item();
            }
            Ok(Err(err)) | Err(err) => {
                result = Err(err);
                break;
            }
        }
    }
    repoint::apply(&state, &changes).await;
    let deleted = job.progress.items.load(Ordering::Relaxed);
    finish(&state, &job, log_id, &result, (deleted, 0)).await;
}

/// 自分のコピー・移動・削除の処理。走っているものと、終わってから少しの間のもの。
#[utoipa::path(
    get,
    path = "/shares/jobs",
    responses(
        (status = OK, body = Vec<ShareJobResponse>, description = "処理の一覧"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
    )
)]
async fn list_jobs(user: AuthUser, State(state): State<AppState>) -> Json<Vec<ShareJobResponse>> {
    Json(
        state
            .share_jobs
            .list(user.id)
            .iter()
            .map(|job| job.snapshot())
            .collect(),
    )
}

/// 処理の進み具合。
#[utoipa::path(
    get,
    path = "/shares/jobs/{id}",
    params(("id" = String, Path)),
    responses(
        (status = OK, body = ShareJobResponse, description = "進み具合"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "無い (ほかの人の処理、終わってから時間がたったもの)"),
    )
)]
async fn get_job(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ShareJobResponse>, AppError> {
    let job = state
        .share_jobs
        .get(user.id, &id)
        .ok_or(AppError::NotFound)?;
    Ok(Json(job.snapshot()))
}

/// 処理を中止する。済んだ分はそのまま残り、途中のファイル (写しかけ) は残さない。上書きの途中なら、今あるものも残る。
/// 終わっていれば何もしない。
#[utoipa::path(
    delete,
    path = "/shares/jobs/{id}",
    params(("id" = String, Path)),
    responses(
        (status = NO_CONTENT, description = "中止を頼んだ"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "無い"),
    )
)]
async fn cancel_job(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let job = state
        .share_jobs
        .get(user.id, &id)
        .ok_or(AppError::NotFound)?;
    job.progress.cancel();
    Ok(StatusCode::NO_CONTENT)
}

pub(super) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(check_transfer))
        .routes(routes!(start_transfer))
        .routes(routes!(check_delete))
        .routes(routes!(start_delete))
        .routes(routes!(list_jobs))
        .routes(routes!(get_job, cancel_job))
}
