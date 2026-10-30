import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { POLL_MS, SHOW_AFTER_MS, ShareJobsState, jobProgressRatio } from './share-jobs.svelte';

const { get, del } = vi.hoisted(() => ({ get: vi.fn(), del: vi.fn() }));
vi.mock('$lib/api/client', () => ({ client: { GET: get, DELETE: del } }));

function job(state: 'running' | 'succeeded' | 'failed', doneItems = 0) {
	return {
		id: 'j1',
		op: 'copy' as const,
		state,
		doneItems,
		totalItems: 4,
		doneBytes: 0,
		totalBytes: 0,
		skipped: 0
	};
}

function ok(data: unknown) {
	return { data, response: new Response(null, { status: 200 }) };
}

describe('ShareJobsState', () => {
	beforeEach(() => {
		vi.useFakeTimers();
	});
	afterEach(() => {
		vi.useRealTimers();
		get.mockReset();
		del.mockReset();
	});

	it('shows a job only once it has run for a second', async () => {
		get.mockResolvedValue(ok(job('running', 1)));
		const jobs = new ShareJobsState();
		jobs.track('j1', 'copy', 'Copying “a”');

		await vi.advanceTimersByTimeAsync(SHOW_AFTER_MS - 1);
		expect(jobs.visible).toHaveLength(0);
		await vi.advanceTimersByTimeAsync(1);
		expect(jobs.visible).toHaveLength(1);
		expect(jobs.visible[0].job.doneItems).toBe(1);
	});

	it('stops tracking a finished job, reports it and refreshes', async () => {
		get.mockResolvedValue(ok(job('succeeded', 4)));
		const jobs = new ShareJobsState();
		const finished = vi.fn();
		jobs.onfinish = finished;
		const ondone = vi.fn();
		jobs.track('j1', 'copy', 'Copying “a”', ondone);

		await vi.advanceTimersByTimeAsync(POLL_MS);
		expect(jobs.jobs).toHaveLength(0);
		expect(ondone).toHaveBeenCalledTimes(1);
		expect(finished).toHaveBeenCalledWith({ job: job('succeeded', 4), label: 'Copying “a”' });

		// 終わった後は読み直さない。
		await vi.advanceTimersByTimeAsync(POLL_MS * 4);
		expect(get).toHaveBeenCalledTimes(1);
	});

	it('asks the server to cancel', async () => {
		get.mockResolvedValue(ok(job('running')));
		del.mockResolvedValue({ response: new Response(null, { status: 204 }) });
		const jobs = new ShareJobsState();
		jobs.track('j1', 'copy', 'Copying “a”');

		await jobs.cancel('j1');
		expect(del).toHaveBeenCalledWith('/api/v1/shares/jobs/{id}', {
			params: { path: { id: 'j1' } }
		});
		expect(jobs.jobs[0].cancelling).toBe(true);
	});
});

describe('jobProgressRatio', () => {
	it('measures by bytes when known, else by items', () => {
		expect(jobProgressRatio({ ...job('running', 1), totalBytes: 100, doneBytes: 25 })).toBe(0.25);
		expect(jobProgressRatio(job('running', 1))).toBe(0.25);
		expect(jobProgressRatio({ ...job('running'), totalItems: 0 })).toBe(0);
	});
});
