/**
 * ページ内で開くもの (画像・PDF・動画・テキストなど) の並びと、その間の前後の移動
 * (→ docs/ui.md「PDF・動画・テキストのビューア」)。
 *
 * 画像は PhotoSwipe (`$lib/image-viewer.ts`)、ほかは `file-viewer.svelte` で開き、ビューアは2つのまま、
 * 前後だけを一覧の表示順でつなぐ。隣が別の種類なら、今のビューアから相手のビューアへ渡す。
 */

import { flushSync } from 'svelte';
import { fileViewer, type ViewerFile } from '$lib/file-viewer.svelte';
import { openImageViewer, type ViewerImage } from '$lib/image-viewer';

export type ViewerItem = { type: 'image'; image: ViewerImage } | { type: 'file'; file: ViewerFile };

/** 一覧の行から、ページ内で開くものを表示順に並べる。行の出し分けと同じく、画像を先に見る。 */
export function viewerItems<T>(
	entries: T[],
	toImage: (entry: T) => ViewerImage | undefined,
	toFile: (entry: T) => ViewerFile | undefined
): ViewerItem[] {
	return entries.flatMap((entry): ViewerItem[] => {
		const image = toImage(entry);
		if (image) return [{ type: 'image', image }];
		const file = toFile(entry);
		return file ? [{ type: 'file', file }] : [];
	});
}

function itemSrc(item: ViewerItem): string {
	return item.type === 'image' ? item.image.src : item.file.src;
}

/** 行から開く。`item` は `items` と同じ組み立て方で作る (押した行を `items` の中から `src` で探すため)。 */
export function openViewerItem(
	item: ViewerItem,
	items: ViewerItem[],
	signal: AbortSignal
): Promise<void> {
	const at = items.findIndex((candidate) => itemSrc(candidate) === itemSrc(item));
	return openAt(at < 0 ? [item] : items, Math.max(at, 0), signal);
}

function openAt(items: ViewerItem[], index: number, signal: AbortSignal): Promise<void> {
	if (items[index].type === 'image') {
		return openImageViewer(items, index, signal, {
			// 画像の並びの端から、隣のファイルへ。ファイルのビューアを先に開いてから画像のビューアを閉じる。
			onLeave: (target) => showFile(items, target)
		});
	}
	showFile(items, index);
	return Promise.resolve();
}

function showFile(items: ViewerItem[], index: number) {
	fileViewer.show(items, index, (target) => {
		// ファイルから隣の画像へ。画像を読み終えるまでファイルを出したままにし、間にページを見せない。
		// 2つのビューアがどちらもフォーカスを中に閉じ込めるので、画像のビューアを開く直前に閉じ切る。
		void openImageViewer(items, target, new AbortController().signal, {
			animate: false,
			beforeOpen: () => flushSync(() => (fileViewer.open = false)),
			onLeave: (next) => showFile(items, next)
		});
	});
}
