import { tick } from 'svelte';
import type { WindowVirtualizerHandle } from 'virtua/svelte';

/**
 * 1ページに並べる段の数。要素の高さには上限があり (Firefox で約1,790万 px)、1段の高さによって25万〜40万段ほどで
 * 最後まで届かなくなるので、10万段ごとにページを分ける (→ docs/archive.md「画面が見える辺りを取りに行く」)。
 */
export const PAGE_LINES = 100_000;

/**
 * 見えている所だけを描く長い一覧を、`PAGE_LINES` 段ごとのページに分ける。画面は `virtual-page-nav.svelte` を置く。
 *
 * コンポーネントの初期化の中で作ること (`$effect` を登録するため)。
 */
export class VirtualPages {
	page = $state(0);
	pageCount: number;
	/** 今のページの先頭の段の番号。 */
	start: number;
	/** 今のページに並べる段の番号。 */
	lines: number[];

	/** @param totalLines 一覧の段の数。 */
	constructor(totalLines: () => number) {
		this.pageCount = $derived(Math.max(1, Math.ceil(totalLines() / PAGE_LINES)));
		this.start = $derived(this.page * PAGE_LINES);
		this.lines = $derived(
			Array.from(
				{ length: Math.max(0, Math.min(PAGE_LINES, totalLines() - this.start)) },
				(_, i) => this.start + i
			)
		);
		// 一覧を取り直したり段の組み方が変わったりして、今のページが無くなったら最後のページにする。
		$effect(() => {
			if (this.page >= this.pageCount) this.page = this.pageCount - 1;
		});
	}

	/** ページを移り、その先頭から見せる。下のボタンで移ったときに、新しいページの途中から始まらないように。 */
	async goTo(next: number, virtualizer: WindowVirtualizerHandle | undefined): Promise<void> {
		this.page = next;
		await tick();
		virtualizer?.scrollToIndex(0);
	}
}
