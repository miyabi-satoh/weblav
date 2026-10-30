import { client } from '$lib/api/client';
import { MESSAGES, GENERIC_ERROR_MESSAGE } from '$lib/api/errors';
import * as m from '$lib/paraglide/messages.js';
import type { components } from '$lib/api/schema';

type ShareJob = components['schemas']['ShareJobResponse'];
type JobOp = components['schemas']['JobOp'];

/**
 * 共有の中のコピー・移動・削除は、サーバーの裏で続く処理になる (→ docs/shares.md)。
 * 始めた画面を離れても進み具合を追えるよう、ルートのレイアウトの部品 (`share-jobs-panel.svelte`) が
 * ここを読んで出す。すぐ終わる処理で欄がちらつかないよう、1秒たってもまだ走っているものだけを出す。
 */
export const SHOW_AFTER_MS = 1000;
/** 進み具合を読み直す間隔。1つの数を読むだけの軽い要求なので、動いて見える程度に短くする。 */
export const POLL_MS = 500;

export type TrackedJob = {
	job: ShareJob;
	/** 欄に出す文言 (「「a」をコピーしています」)。 */
	label: string;
	/** 欄に出すか。始めてから `SHOW_AFTER_MS` たってもまだ走っているもの。 */
	shown: boolean;
	cancelling: boolean;
};

/** 終わった処理の知らせ。欄を出す部品が、失敗はダイアログで、飛ばした件数はトーストで伝える。 */
export type FinishedJob = { job: ShareJob; label: string };

/** 処理の種類ごとの、名前の分からないときの文言 (読み込み直して拾ったもの)。 */
export function jobLabel(op: JobOp, name?: string): string {
	if (name === undefined) {
		if (op === 'copy') return m.shares_job_copy_generic();
		if (op === 'move') return m.shares_job_move_generic();
		return m.shares_job_delete_generic();
	}
	if (op === 'copy') return m.shares_job_copy({ name });
	if (op === 'move') return m.shares_job_move({ name });
	return m.shares_job_delete({ name });
}

/** 進み具合の文言。数が分かる前 (0 件) は件数を出さない。 */
export function jobProgressText(job: ShareJob): string {
	if (job.totalItems === 0) return '';
	return m.shares_job_progress({ done: job.doneItems, total: job.totalItems });
}

/** 進み具合の割合 (0〜1)。大きさが分かればバイトで、分からなければ件数で測る。 */
export function jobProgressRatio(job: ShareJob): number {
	if (job.totalBytes > 0) return Math.min(1, job.doneBytes / job.totalBytes);
	if (job.totalItems > 0) return Math.min(1, job.doneItems / job.totalItems);
	return 0;
}

/** 終わった処理の失敗の文言。中止は失敗として扱わない。 */
export function jobErrorMessage(job: ShareJob): string | undefined {
	if (job.state !== 'failed') return undefined;
	return job.error ? MESSAGES[job.error]() : GENERIC_ERROR_MESSAGE();
}

export class ShareJobsState {
	jobs = $state<TrackedJob[]>([]);
	/** 終わった処理を、欄を出す部品が受け取る。 */
	onfinish: ((finished: FinishedJob) => void) | undefined = undefined;
	// 画面には出さない後始末の控えなので、SvelteMap にして変化を追わせる必要は無い。
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- 上のとおり
	#doneCallbacks = new Map<string, () => void | Promise<void>>();
	#timer: ReturnType<typeof setTimeout> | undefined;

	/** 欄に出す処理。 */
	get visible(): TrackedJob[] {
		return this.jobs.filter((tracked) => tracked.shown);
	}

	/**
	 * 始めた処理を追う。終わったら (失敗・中止でも、済んだ分はあるので) `ondone` を呼ぶ。
	 * 呼び出し側は、そこで一覧を取り直す。
	 */
	track(jobId: string, op: JobOp, label: string, ondone?: () => void | Promise<void>) {
		if (this.jobs.some((tracked) => tracked.job.id === jobId)) return;
		if (ondone) this.#doneCallbacks.set(jobId, ondone);
		this.jobs.push({
			job: {
				id: jobId,
				op,
				state: 'running',
				doneItems: 0,
				totalItems: 0,
				doneBytes: 0,
				totalBytes: 0,
				skipped: 0
			},
			label,
			shown: false,
			cancelling: false
		});
		setTimeout(() => {
			const tracked = this.jobs.find((candidate) => candidate.job.id === jobId);
			if (tracked) tracked.shown = true;
		}, SHOW_AFTER_MS);
		this.#schedule();
	}

	/**
	 * 読み込み直す前から走っている自分の処理を拾う。画面を移っても欄が消えないのは同じレイアウトの中だけなので、
	 * 読み込み直したときに拾い直す。終わってから少しの間のものも返るが、それは拾わない。
	 */
	async resume(ondone?: () => void | Promise<void>) {
		try {
			const { data, response } = await client.GET('/api/v1/shares/jobs');
			if (!response.ok || !data) return;
			for (const job of data) {
				if (job.state !== 'running') continue;
				this.track(job.id, job.op, jobLabel(job.op), ondone);
			}
		} catch {
			// 拾えなくても、処理はサーバーで続く。
		}
	}

	async cancel(jobId: string) {
		const tracked = this.jobs.find((candidate) => candidate.job.id === jobId);
		if (!tracked || tracked.cancelling) return;
		tracked.cancelling = true;
		try {
			const { response } = await client.DELETE('/api/v1/shares/jobs/{id}', {
				params: { path: { id: jobId } }
			});
			// 頼めなければ処理は続くので、押し直せるように戻す。止まれば次の読み直しで欄から消える。
			if (!response.ok) tracked.cancelling = false;
		} catch {
			tracked.cancelling = false;
		}
	}

	#schedule() {
		if (this.#timer !== undefined) return;
		this.#timer = setTimeout(() => {
			this.#timer = undefined;
			void this.#poll();
		}, POLL_MS);
	}

	async #poll() {
		const ids = this.jobs.map((tracked) => tracked.job.id);
		await Promise.all(ids.map((id) => this.#refresh(id)));
		if (this.jobs.length > 0) this.#schedule();
	}

	async #refresh(jobId: string) {
		let job: ShareJob | undefined;
		try {
			const { data, response } = await client.GET('/api/v1/shares/jobs/{id}', {
				params: { path: { id: jobId } }
			});
			// 無くなった (サーバーを起動し直した) ものは、追うのをやめる。
			if (response.status === 404) {
				this.#finish(jobId, undefined);
				return;
			}
			if (!response.ok || !data) return;
			job = data;
		} catch {
			// 次の読み直しで取り直す。
			return;
		}
		const tracked = this.jobs.find((candidate) => candidate.job.id === jobId);
		if (!tracked) return;
		tracked.job = job;
		if (job.state !== 'running') this.#finish(jobId, job);
	}

	#finish(jobId: string, job: ShareJob | undefined) {
		const tracked = this.jobs.find((candidate) => candidate.job.id === jobId);
		this.jobs = this.jobs.filter((candidate) => candidate.job.id !== jobId);
		const ondone = this.#doneCallbacks.get(jobId);
		this.#doneCallbacks.delete(jobId);
		if (tracked && job) this.onfinish?.({ job, label: tracked.label });
		void ondone?.();
	}
}

export const shareJobs = new ShareJobsState();
