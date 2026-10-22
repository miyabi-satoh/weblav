import { toast } from 'svelte-sonner';
import { client } from '$lib/api/client';
import { GENERIC_ERROR_MESSAGE, errorDetail, errorMessage } from '$lib/api/errors';
import type { components } from '$lib/api/schema';
import { formatNumber } from '$lib/format';
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

/**
 * 再スキャンの間、進み具合をトーストに出す (→ docs/archive.md「スキャン」)。返した関数で止めて消す。
 * 最初の問い合わせを間隔の後にするので、すぐ終わるスキャンでは何も出さない。
 */
function showProgress(id: number): () => void {
	let toastId: string | number | undefined;
	let stopped = false;
	const tick = async () => {
		try {
			const { data } = await client.GET('/api/v1/contents/{id}/rescan', {
				params: { path: { id } }
			});
			if (stopped || !data?.phase) return;
			const message =
				data.phase === 'listing'
					? m.archive_scan_listing_toast()
					: m.archive_scan_reading_toast({
							done: formatNumber(data.done),
							total: formatNumber(data.total)
						});
			toastId = toast.loading(message, { id: toastId });
		} catch {
			// 進み具合が見られなくても、スキャンそのものは続く。
		}
	};
	const timer = setInterval(tick, PROGRESS_INTERVAL_MS);
	return () => {
		stopped = true;
		clearInterval(timer);
		if (toastId !== undefined) toast.dismiss(toastId);
	};
}

/**
 * アーカイブを再スキャンする。失敗は投げずに、画面に出す文言にして返す。
 *
 * 作成・場所の変更の直後にも呼ぶ (→ docs/archive.md「スキャン」)。その場合は作成・保存が
 * 済んだ後の失敗なので、呼び出し側が「保存はできた」ことと分けて伝える。
 */
export async function rescanArchive(id: number): Promise<ArchiveScanResult> {
	const stopProgress = showProgress(id);
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
