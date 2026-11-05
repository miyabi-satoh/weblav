/**
 * 編集ページの再スキャンの状態 (→ docs/ui.md「コンテンツの編集ページ」)。ボタンと進み具合は見出しの段にあるので、
 * タブを移っても消えないようレイアウトが持ち、タブの画面は終わったことだけを受け取る。
 */
import { getContext, setContext } from 'svelte';
import { refreshAll } from '$app/navigation';
import { type ArchiveScanResult, rescanArchive, waitForRunningScan } from '$lib/archive-scan';
import { formatNumber } from '$lib/format';
import * as m from '$lib/paraglide/messages.js';

const CONTEXT_KEY = Symbol('archive-rescan');

export class ArchiveRescanState {
	/** 走っているスキャンの進み具合。見出しの下に出す。 */
	progress = $state<string | undefined>();
	/** 最後の再スキャンの結果。次の再スキャンまで残す。 */
	result = $state<string | null>(null);
	/** ボタンから押せないか。保存に続くスキャンの間も押させない (押すと 409 になるため)。 */
	busy = $state(false);

	/** ボタンからの再スキャン。結果は見出しの下に出す。失敗は `onerror` に渡す。 */
	async run(id: number, onerror: (message: string) => void): Promise<void> {
		if (this.busy) return;
		this.busy = true;
		// 失敗したときに、前回の成功の件数を今回の結果と読み違えさせない。
		this.result = null;
		try {
			const scan = await rescanArchive(id, (message) => (this.progress = message));
			if (!scan.ok) {
				onerror(scan.message);
				return;
			}
			this.result = scan.result.cancelled
				? m.archive_items_rescan_cancelled()
				: m.archive_items_rescan_result({
						added: formatNumber(scan.result.added),
						removed: formatNumber(scan.result.removed)
					});
			await this.#reload();
		} finally {
			this.busy = false;
		}
	}

	/** 保存に続くスキャン。進み具合と結果は保存した所で伝えるので、ここには出さない。 */
	async runAfterSave(
		id: number,
		onProgress: (message: string | undefined) => void
	): Promise<ArchiveScanResult> {
		this.busy = true;
		try {
			return await rescanArchive(id, onProgress);
		} finally {
			this.busy = false;
		}
	}

	/**
	 * 開いたときにスキャンが走っていれば、終わるまで押させずに進み具合を出し、終わったら取り直す。
	 * スキャンは画面を閉じても続くので、開き直した画面がそれを知らないと、押し直して 409 になるため。
	 */
	async watch(id: number, signal: AbortSignal): Promise<void> {
		const finished = await waitForRunningScan(
			id,
			(message) => {
				this.progress = message;
				this.busy = message !== undefined;
			},
			signal
		);
		if (finished) await this.#reload();
	}

	/** 別のコンテンツへ移ったときに、前のコンテンツの結果を残さない。 */
	reset() {
		this.progress = undefined;
		this.result = null;
		this.busy = false;
	}

	async #reload() {
		await refreshAll();
	}
}

export function provideArchiveRescan(state: ArchiveRescanState) {
	setContext(CONTEXT_KEY, state);
}

export function archiveRescan(): ArchiveRescanState {
	return getContext<ArchiveRescanState>(CONTEXT_KEY);
}
