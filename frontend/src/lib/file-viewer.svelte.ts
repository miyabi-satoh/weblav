/**
 * ページ内で PDF・動画・テキストなどを重ねて表示するビューアの状態 (→ docs/ui.md「PDF・動画・テキストのビューア」)。
 * 画面に置くのは `file-viewer.svelte` の1つだけで、一覧の行はここを通して開く。
 */

import type { ViewerFileKind } from '$lib/file-kind';

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

let files = $state<ViewerFile[]>([]);
let index = $state(0);
let open = $state(false);

export const fileViewer = {
	get open() {
		return open;
	},
	set open(value: boolean) {
		open = value;
	},
	get files() {
		return files;
	},
	get index() {
		return index;
	},
	get current(): ViewerFile | undefined {
		return files[index];
	},
	/** `file` を開く。前後は `list` (開いた一覧に並ぶ、ビューアで開くファイルの表示順)。 */
	show(file: ViewerFile, list: ViewerFile[]) {
		const at = list.findIndex((candidate) => candidate.src === file.src);
		files = at < 0 ? [file] : list;
		index = Math.max(at, 0);
		open = true;
	},
	previous() {
		if (index > 0) index -= 1;
	},
	next() {
		if (index < files.length - 1) index += 1;
	}
};
