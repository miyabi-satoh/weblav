import type { Attachment } from 'svelte/attachments';

/**
 * CSS grid の子を、上から順に空きのいちばん上 (いちばん短い列) へ詰める (石垣の並べ方)。
 * 列の数と列の間は、付ける要素の class (`grid-cols-*`・`gap-*`) で決める。子の上下の間も列の間と同じにする。
 *
 * FIX: 同じ並べ方の CSS (`display: grid-lanes`) は Safari にしか無い。
 * 行を 1px にし、子ごとに高さの分だけ行をまたがせ、`dense` で上の空きから埋めさせて同じ形にする。
 */
export const masonry: Attachment<HTMLElement> = (grid) => {
	grid.style.gridAutoRows = '1px';
	grid.style.gridAutoFlow = 'row dense';
	grid.style.rowGap = '0px';
	// 子を行いっぱいに伸ばすと、測る高さが中身でなく、またいだ行の高さになるため。
	grid.style.alignItems = 'start';

	function place(item: Element) {
		if (!(item instanceof HTMLElement)) return;
		const gap = parseFloat(getComputedStyle(grid).columnGap) || 0;
		const height = item.getBoundingClientRect().height;
		item.style.gridRowEnd = `span ${Math.ceil(height + gap)}`;
	}

	const resize = new ResizeObserver((entries) => {
		for (const entry of entries) place(entry.target);
	});
	function observe(children: Iterable<Node>) {
		for (const child of children) {
			if (child instanceof HTMLElement) resize.observe(child);
		}
	}
	observe(grid.children);
	// 取得を待って出す区画のように、後から加わる子も並べる。
	const mutation = new MutationObserver((records) => {
		for (const record of records) observe(record.addedNodes);
	});
	mutation.observe(grid, { childList: true });

	return () => {
		resize.disconnect();
		mutation.disconnect();
		grid.style.gridAutoRows = '';
		grid.style.gridAutoFlow = '';
		grid.style.rowGap = '';
		grid.style.alignItems = '';
		for (const child of grid.children) {
			if (child instanceof HTMLElement) child.style.gridRowEnd = '';
		}
	};
};
