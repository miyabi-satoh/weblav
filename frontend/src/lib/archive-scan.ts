import { client } from '$lib/api/client';
import { GENERIC_ERROR_MESSAGE, errorDetail, errorMessage } from '$lib/api/errors';
import type { components } from '$lib/api/schema';
import { formatDuration, formatNumber } from '$lib/format';
import * as m from '$lib/paraglide/messages.js';

type RescanResponse = components['schemas']['RescanResponse'];

export type ArchiveScanResult =
	{ ok: true; result: RescanResponse } | { ok: false; message: string; retryable: boolean };

/** フォルダーが無い・読めない・多すぎる失敗は、直すまで再スキャンしても同じになる。 */
const NOT_RETRYABLE = new Set([
	'archiveFolderMissing',
	'archiveFolderUnreadable',
	'archiveTooManyFiles'
]);

/**
 * 進み具合を見に行く間隔 (ms)。数万件を読む NAS では1件ずつ数十ミリ秒かかるので、1秒ごとでも数字は動いて見える。
 * 1回の問い合わせはサーバーの手元の数を返すだけなので、この間隔で重ねても負荷にならない。
 */
const PROGRESS_INTERVAL_MS = 1000;

/** 走っているスキャンの進み具合の文言。走っていなければ `null`、見られなければ `undefined`。 */
async function fetchProgress(id: number): Promise<string | null | undefined> {
	try {
		const { data } = await client.GET('/api/v1/contents/{id}/rescan', {
			params: { path: { id } }
		});
		if (!data) return undefined;
		if (!data.phase) return null;
		const elapsed = formatDuration(data.elapsedSeconds);
		return data.phase === 'listing'
			? m.archive_scan_listing_progress({ elapsed })
			: m.archive_scan_reading_progress({
					done: formatNumber(data.done),
					total: formatNumber(data.total),
					elapsed
				});
	} catch {
		// 進み具合が見られなくても、スキャンそのものは続く。
		return undefined;
	}
}

/**
 * スキャンの進み具合を、押したボタンの近くに出す文言として渡し続ける (→ docs/archive.md「スキャン」)。
 * 返した関数で止めると、`undefined` を渡して消させる。
 * 最初の問い合わせを間隔の後にするので、すぐ終わるスキャンでは何も出さない。
 */
function watchProgress(id: number, onProgress: (message: string | undefined) => void): () => void {
	let stopped = false;
	const timer = setInterval(async () => {
		const message = await fetchProgress(id);
		if (!stopped && message) onProgress(message);
	}, PROGRESS_INTERVAL_MS);
	return () => {
		stopped = true;
		clearInterval(timer);
		onProgress(undefined);
	};
}

/**
 * 走っているスキャンがあれば、終わるまで進み具合を渡し続けて待つ。走り終えたのを見届けたかを返す。
 * スキャンは画面を閉じても続くので、開き直した画面が、終わるまで押させずに待つため。
 * `signal` で止めると、見届けずに `false` を返す (画面を離れた後まで問い合わせ続けないため)。
 */
export async function waitForRunningScan(
	id: number,
	onProgress: (message: string | undefined) => void,
	signal: AbortSignal
): Promise<boolean> {
	let message = await fetchProgress(id);
	if (!message) return false;
	try {
		while (message !== null) {
			if (signal.aborted) return false;
			if (message) onProgress(message);
			await new Promise((resolve) => setTimeout(resolve, PROGRESS_INTERVAL_MS));
			message = await fetchProgress(id);
		}
		return !signal.aborted;
	} finally {
		onProgress(undefined);
	}
}

/**
 * アーカイブを再スキャンする。失敗は投げずに、画面に出す文言にして返す。
 *
 * 作成・場所の変更の直後にも呼ぶ (→ docs/archive.md「スキャン」)。その場合は作成・保存が
 * 済んだ後の失敗なので、呼び出し側が「保存はできた」ことと分けて伝える。
 */
export async function rescanArchive(
	id: number,
	onProgress: (message: string | undefined) => void
): Promise<ArchiveScanResult> {
	const stopProgress = watchProgress(id, onProgress);
	try {
		const { data, error, response } = await client.POST('/api/v1/contents/{id}/rescan', {
			params: { path: { id } }
		});
		if (!response.ok || !data) {
			const kind = errorDetail(error)?.kind;
			return {
				ok: false,
				message: errorMessage(error),
				retryable: kind === undefined || !NOT_RETRYABLE.has(kind)
			};
		}
		return { ok: true, result: data };
	} catch {
		return { ok: false, message: GENERIC_ERROR_MESSAGE(), retryable: true };
	} finally {
		stopProgress();
	}
}
