//! 共有の中のコピー・移動・削除を、要求から切り離して裏で続ける処理の置き場。
//!
//! 大きなフォルダーのコピーは数分かかりうるので、要求はすぐに返し (202)、画面は進み具合を読みに来る。
//! 処理はメモリの中にしか無く、起動し直すと消える (記録は失敗にする。→ `log::on_startup`)。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use utoipa::ToSchema;

use crate::error::{AppError, ErrorCode};

use super::fsops::TransferProgress;

/// 終わった処理を残しておく時間。画面を閉じて開き直しても、結果を出せる長さ。
const FINISHED_KEEP: Duration = Duration::from_secs(10 * 60);

/// 処理の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum JobOp {
    Copy,
    Move,
    Delete,
}

/// 処理の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Running,
    Succeeded,
    Failed,
    /// 途中で中止した。それまでに済んだ分はそのまま。
    Cancelled,
}

/// 終わった処理の結果。
struct Outcome {
    state: JobState,
    error: Option<ErrorCode>,
    at: Instant,
}

/// 1つの処理。
pub(super) struct Job {
    pub(super) id: String,
    owner: i64,
    op: JobOp,
    /// 済んだ分と中止の合図。処理のスレッドが書き、進み具合の口が読む。
    pub(super) progress: TransferProgress,
    total_items: u64,
    total_bytes: u64,
    /// 置き先に同じ名前があって飛ばした数。
    skipped: AtomicU64,
    outcome: Mutex<Option<Outcome>>,
}

impl Job {
    pub(super) fn skip(&self) {
        self.skipped.fetch_add(1, Ordering::Relaxed);
    }

    /// 終わったことを書く。
    pub(super) fn finish(&self, result: &Result<(), AppError>) {
        let (state, error) = match result {
            Ok(()) if self.progress.is_cancelled() => (JobState::Cancelled, None),
            Ok(()) => (JobState::Succeeded, None),
            Err(err) => (JobState::Failed, Some(err.code())),
        };
        *self.outcome.lock().expect("share job lock poisoned") = Some(Outcome {
            state,
            error,
            at: Instant::now(),
        });
    }

    fn finished_at(&self) -> Option<Instant> {
        self.outcome
            .lock()
            .expect("share job lock poisoned")
            .as_ref()
            .map(|outcome| outcome.at)
    }

    pub(super) fn snapshot(&self) -> ShareJobResponse {
        let outcome = self.outcome.lock().expect("share job lock poisoned");
        let (state, error) = match outcome.as_ref() {
            Some(outcome) => (outcome.state, outcome.error),
            None => (JobState::Running, None),
        };
        ShareJobResponse {
            id: self.id.clone(),
            op: self.op,
            state,
            done_items: self.progress.items.load(Ordering::Relaxed),
            total_items: self.total_items,
            done_bytes: self.progress.bytes.load(Ordering::Relaxed),
            total_bytes: self.total_bytes,
            skipped: self.skipped.load(Ordering::Relaxed),
            error,
        }
    }
}

/// 処理の進み具合。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShareJobResponse {
    id: String,
    op: JobOp,
    state: JobState,
    /// 済んだファイル・フォルダーの数。削除では、選んだもののうち消し終えた数。
    done_items: u64,
    /// 扱うファイル・フォルダーの数 (中のものも数える)。削除では、選んだものの数。
    total_items: u64,
    /// 写し終えたバイト数。移動と削除では、済んだものの大きさ。
    done_bytes: u64,
    /// 扱うファイルの大きさの合計。削除では 0 (数えない)。
    total_bytes: u64,
    /// 置き先に同じ名前があって飛ばした数。
    skipped: u64,
    /// 失敗したときの API のエラーの code。
    error: Option<ErrorCode>,
}

impl ShareJobResponse {
    pub(super) fn done_items(&self) -> u64 {
        self.done_items
    }

    pub(super) fn done_bytes(&self) -> u64 {
        self.done_bytes
    }
}

/// 処理の一覧。ユーザーは自分の処理だけを見られる。
#[derive(Default)]
pub struct ShareJobs {
    // `std::sync::Mutex` を使う。持つのは表を触る一瞬だけで、持ったまま await しないため。
    jobs: Mutex<HashMap<String, Arc<Job>>>,
}

impl ShareJobs {
    /// 新しい処理を登録する。
    pub(super) fn start(
        &self,
        owner: i64,
        op: JobOp,
        total_items: u64,
        total_bytes: u64,
    ) -> Result<Arc<Job>, AppError> {
        let job = Arc::new(Job {
            id: crate::api::random_hex::<8>()?,
            owner,
            op,
            progress: TransferProgress::default(),
            total_items,
            total_bytes,
            skipped: AtomicU64::new(0),
            outcome: Mutex::new(None),
        });
        let mut jobs = self.jobs.lock().expect("share jobs lock poisoned");
        prune(&mut jobs);
        jobs.insert(job.id.clone(), Arc::clone(&job));
        Ok(job)
    }

    /// `owner` の処理 `id`。ほかの人の処理は無いものとする。
    pub(super) fn get(&self, owner: i64, id: &str) -> Option<Arc<Job>> {
        let mut jobs = self.jobs.lock().expect("share jobs lock poisoned");
        prune(&mut jobs);
        jobs.get(id).filter(|job| job.owner == owner).cloned()
    }

    /// `owner` の処理。走っているものと、終わってから `FINISHED_KEEP` 以内のもの。
    pub(super) fn list(&self, owner: i64) -> Vec<Arc<Job>> {
        let mut jobs = self.jobs.lock().expect("share jobs lock poisoned");
        prune(&mut jobs);
        jobs.values()
            .filter(|job| job.owner == owner)
            .cloned()
            .collect()
    }
}

fn prune(jobs: &mut HashMap<String, Arc<Job>>) {
    jobs.retain(|_, job| {
        job.finished_at()
            .is_none_or(|at| at.elapsed() < FINISHED_KEEP)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_is_visible_only_to_its_owner() {
        let jobs = ShareJobs::default();
        let job = jobs.start(1, JobOp::Copy, 3, 10).expect("始められるはず");
        assert!(jobs.get(1, &job.id).is_some());
        assert!(jobs.get(2, &job.id).is_none());
        assert_eq!(jobs.list(2).len(), 0);
        assert_eq!(jobs.list(1).len(), 1);
    }

    #[test]
    fn a_finished_job_reports_how_it_ended() {
        let jobs = ShareJobs::default();
        let job = jobs.start(1, JobOp::Delete, 1, 0).expect("始められるはず");
        assert_eq!(job.snapshot().state, JobState::Running);
        job.finish(&Err(AppError::TrashUnavailable));
        let snapshot = job.snapshot();
        assert_eq!(snapshot.state, JobState::Failed);
        assert!(matches!(snapshot.error, Some(ErrorCode::TrashUnavailable)));

        let cancelled = jobs.start(1, JobOp::Copy, 1, 0).expect("始められるはず");
        cancelled.progress.cancel();
        cancelled.finish(&Ok(()));
        assert_eq!(cancelled.snapshot().state, JobState::Cancelled);
    }
}
