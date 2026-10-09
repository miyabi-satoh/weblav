/**
 * ページ内で PDF・動画・テキストなどを重ねて表示するビューアーの状態 (→ docs/ui.md「PDF・動画・テキストのビューアー」)。
 * 画面に置くのは `file-viewer.svelte` の1つだけ。前後には画像も並び、隣が画像なら画像のビューアーへ渡す。
 */

import type { ViewerFileKind } from '$lib/file-kind';
import type { ViewerItem } from '$lib/viewer-items';

export type ViewerFile = {
	/** ダウンロードの URL。`embed` では、動画サイトの埋め込みプレイヤーの URL (→ $lib/video-embed.ts)。 */
	src: string;
	title: string;
	/** 元のファイル名。テキストに色を付けるかを拡張子で決める (→ $lib/code-highlight.ts)。 */
	fileName: string;
	kind: ViewerFileKind;
	/**
	 * 行に出す縮小画像 (→ docs/ui.md「画像のプレビュー」)。`src` は縮小画像、`original` は元のファイルの URL。
	 * 読めなければ行がアイコンに戻す。
	 */
	thumbnail?: { src: string; original: string };
	/**
	 * URL のファイルの元の URL (→ docs/ui.md「URL のファイル」)。「新しいタブで開く」は、`src` (中継) でなく
	 * これを開く。中継できない相手 (名前を引くと LAN を指すものなど) も、閲覧する端末からは開けるため。
	 */
	originalUrl?: string;
};

let items = $state<ViewerItem[]>([]);
let index = $state(0);
let open = $state(false);
/**
 * 隣が画像のとき、画像のビューアーへ渡す (→ $lib/viewer-items.ts)。画像を読み終えるまで待つので、
 * 待つ間に閉じたら (Esc・画面の移動など) `signal` を中断し、後から画像のビューアーを出さない。
 */
let leave: (target: number, signal: AbortSignal) => Promise<void> = async () => {};
/** 画像のビューアーへ渡している途中。待つ間の前後の操作は受けない (押しっぱなしで画像のビューアーが重なるため)。 */
let handoff = $state<AbortController | null>(null);

export const fileViewer = {
	get open() {
		return open;
	},
	set open(value: boolean) {
		if (!value) cancelHandoff();
		open = value;
	},
	/** 隣の画像を読み込んでいる。 */
	get leaving() {
		return handoff !== null;
	},
	/** 開いた一覧に並ぶ、ページ内で開くものの数。画像も数える。 */
	get count() {
		return items.length;
	},
	get index() {
		return index;
	},
	get current(): ViewerFile | undefined {
		const item = items[index];
		return item?.type === 'file' ? item.file : undefined;
	},
	/** `items[at]` (ファイル) を開く。行からは `$lib/viewer-items.ts` の `openViewerItem` を通す。 */
	show(
		list: ViewerItem[],
		at: number,
		onLeave: (target: number, signal: AbortSignal) => Promise<void>
	) {
		cancelHandoff();
		items = list;
		index = at;
		leave = onLeave;
		open = true;
	},
	/** 前後のボタンの読み上げを、行き先の種類に合わせる (画像のビューアーの端の矢印と同じ)。 */
	get previousIsImage() {
		return items[index - 1]?.type === 'image';
	},
	get nextIsImage() {
		return items[index + 1]?.type === 'image';
	},
	previous() {
		if (index > 0) move(index - 1);
	},
	next() {
		if (index < items.length - 1) move(index + 1);
	}
};

function move(target: number) {
	if (handoff) return;
	if (items[target].type === 'file') {
		index = target;
		return;
	}
	const controller = new AbortController();
	handoff = controller;
	void leave(target, controller.signal).finally(() => {
		if (handoff === controller) handoff = null;
	});
}

function cancelHandoff() {
	handoff?.abort();
	handoff = null;
}
