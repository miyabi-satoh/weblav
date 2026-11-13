/**
 * ページ内で開くもの (画像・PDF・動画・テキストなど) の並びと、その間の前後の移動
 * (→ docs/ui.md「PDF・動画・テキストのビューアー」)。
 *
 * 画像は PhotoSwipe (`#lib/image-viewer.ts`)、ほかは `file-viewer.svelte` で開き、ビューアーは2つのまま、
 * 前後だけを一覧の表示順でつなぐ。隣が別の種類なら、今のビューアーから相手のビューアーへ渡す。
 */

import { flushSync } from 'svelte';
import { isCodeFileName } from '#lib/code-highlight.ts';
import type { ContentLocation } from '#lib/content-location.ts';
import { viewerFileKind } from '#lib/file-kind.ts';
import { fileViewer, type ViewerFile } from '#lib/file-viewer.svelte.ts';
import { closeImageViewer, openImageViewer, type ViewerImage } from '#lib/image-viewer.ts';
import { previewClosed, previewShown } from '#lib/preview-url.svelte.ts';

/**
 * `key` は、開いているものを URL の `preview` に持つときの見分け (→ #lib/preview-url.svelte.ts)。
 * 一覧の中で重ならず、読み込み直しても変わらないものを、一覧が持つ値から作る。一覧に無いものは持たない。
 */
export type ViewerItem = (
	{ type: 'image'; image: ViewerImage } | { type: 'file'; file: ViewerFile }
) & { key?: string };

/** 一覧の行から、ページ内で開くものを表示順に並べる。行の出し分けと同じく、画像を先に見る。 */
export function viewerItems<T>(
	entries: T[],
	toImage: (entry: T) => ViewerImage | undefined,
	toFile: (entry: T) => ViewerFile | undefined,
	keyOf: (entry: T) => string
): ViewerItem[] {
	return entries.flatMap((entry): ViewerItem[] => {
		const image = toImage(entry);
		if (image) return [{ type: 'image', image, key: keyOf(entry) }];
		const file = toFile(entry);
		return file ? [{ type: 'file', file, key: keyOf(entry) }] : [];
	});
}

/** `items` の中の、`key` のもの。読み込み直したときに、URL の `preview` から開き直すのに使う。 */
export function findViewerItem(
	items: ViewerItem[],
	key: string
): { items: ViewerItem[]; index: number } | undefined {
	const index = items.findIndex((item) => item.key === key);
	return index < 0 ? undefined : { items, index };
}

function itemSrc(item: ViewerItem): string {
	return item.type === 'image' ? item.image.src : item.file.src;
}

/**
 * 行から開く。`item` は `items` と同じ組み立て方で作る (押した行を `items` の中から `src` で探すため)。
 * `signal` は画像の行が渡す (→ `openImageViewer`)。ファイルはすぐ開くので要らない。
 */
export function openViewerItem(
	item: ViewerItem,
	items: ViewerItem[],
	signal = new AbortController().signal
): Promise<void> {
	cancelRestore();
	const at = items.findIndex((candidate) => itemSrc(candidate) === itemSrc(item));
	return openAt(at < 0 ? [item] : items, Math.max(at, 0), signal);
}

/**
 * Markdown のプレビューの相対パスのリンクから、同じコンテンツの `path` のファイルを開く。
 * 開いた一覧にあればそこへ移り (前後もそのまま送れる)、無ければそのファイルだけをビューアーで開く。
 * ビューアーで開けない種類は、別のタブで開く。
 */
export function openLinkedPath(from: ContentLocation, path: string) {
	const src = from.src(path);
	if (fileViewer.moveToLocation(src)) return;
	const fileName = path.slice(path.lastIndexOf('/') + 1);
	// 一覧に無いファイルは、サーバーが中身を見た `isText` が無いので、拡張子で読めそうなものだけをテキストとして開く。
	const kind = viewerFileKind(fileName, isCodeFileName(fileName));
	if (!kind) {
		window.open(src, '_blank', 'noopener,noreferrer');
		return;
	}
	const file = { src, title: fileName, fileName, kind, location: { path, src: from.src } };
	showFile([{ type: 'file', file }], 0);
}

/** URL の `preview` から開き直す (→ #lib/preview-url.svelte.ts)。 */
export function openViewerAt(items: ViewerItem[], index: number) {
	cancelRestore();
	restoring = new AbortController();
	void openAt(items, index, restoring.signal);
}

/**
 * URL から開き直している途中の画像。読み終えるまで待つ間に画面を移る・別の行を押すと、
 * 移った先や開いたビューアーの上に後から画像が開くので、そのときは取りやめる (行の `pending` と同じ)。
 */
let restoring: AbortController | null = null;

function cancelRestore() {
	restoring?.abort();
	restoring = null;
}

/** 戻る・画面の移動で、開いているビューアーを閉じる。 */
export function closeViewers() {
	cancelRestore();
	fileViewer.open = false;
	closeImageViewer();
}

/** 画像のビューアーが見せているものを URL に写す。 */
const imagePreviewOptions = (items: ViewerItem[]) => ({
	onShow: (index: number) => previewShown(items[index].key ?? null),
	onClose: previewClosed
});

function openAt(items: ViewerItem[], index: number, signal: AbortSignal): Promise<void> {
	if (items[index].type === 'image') {
		return openImageViewer(items, index, signal, {
			...imagePreviewOptions(items),
			// 画像の並びの端から、隣のファイルへ。ファイルのビューアーを先に開いてから画像のビューアーを閉じる。
			onLeave: (target) => showFile(items, target)
		});
	}
	showFile(items, index);
	return Promise.resolve();
}

function showFile(items: ViewerItem[], index: number) {
	fileViewer.show(items, index, (target, signal) =>
		// ファイルから隣の画像へ。画像を読み終えるまでファイルを出したままにし、間にページを見せない。
		// 2つのビューアーがどちらもフォーカスを中に閉じ込めるので、画像のビューアーを開く直前に閉じ切る。
		openImageViewer(items, target, signal, {
			...imagePreviewOptions(items),
			animate: false,
			beforeOpen: () => flushSync(() => (fileViewer.open = false)),
			onLeave: (next) => showFile(items, next)
		})
	);
}
