import { MediaQuery } from 'svelte/reactivity';
import { browseLayout } from '$lib/browse-layout.svelte';

/**
 * 閲覧側の一覧 (トップ・グループ・アーカイブ・フォルダー) で共有する class。
 * 1列リストとタイルの2つの並べ方を持ち、`browse*Class()` が端末で選んだ方を返す (→ $lib/browse-layout.svelte.ts)。
 * 行は枠 (`browseRowClass`) の中に、タイトルを包むリンクかボタン (`browseRowPressClass`) を1つ置き、
 * その押せる範囲を枠いっぱいに広げる。2段目に置く別のボタン (検索の結果の場所) を、リンクの中に入れ子に
 * せずに重ねるため (→ docs/ui.md「UI 全般」)。
 * 行の中身は `list-row-icon.svelte` / `list-row-glyph.svelte`。タイトルのリンク (`<a>` / `<button>`) は
 * 各画面に置く。`resolve()` や `rel="external"` を eslint (svelte/no-navigation-without-resolve) が
 * 静的に見られるようにするため。
 * 管理画面「公開できるフォルダー」(`admin/roots`) は `listClass`/`listItemClass` (枠) だけを使い、
 * 行本体はクリック不可・ホバーなしの一覧なので別に持つ。
 */
export const listClass = 'border-y bg-background';

const listItemBorderClass = 'border-b border-divider';

/** 行の区切り。読み物のページの枠付きリスト (`page-layout.ts`) も同じ引き方にする。 */
export const listItemClass = `${listItemBorderClass} last:border-b-0`;

/** 閲覧側の本文の左右の余白。行と、その外に置く見出し・文言で揃える。 */
export const browseGutterClass = 'px-4 sm:px-6';

const listRowClass = `relative flex min-h-18 w-full items-center gap-4 py-3 text-left transition-colors hover:bg-muted/50 ${browseGutterClass}`;

/** 行の1段目 (タイトル・名前)。 */
const listRowTitleClass = 'block text-lg leading-6';

/** フォルダー一覧用のタイトル。行と同じく、スマートフォン幅では一段小さくする (→ docs/ui.md「UI 全般」)。 */
const compactListRowTitleClass = 'block truncate text-base leading-6 sm:text-lg';

/** 行の2段目 (ホーム・グループの説明、アーカイブの軸の値、フォルダーのサイズと日時)。 */
const listRowSubtitleClass = 'mt-0.5 block text-xs text-muted-foreground sm:text-sm';

/** フォルダー一覧用。名前が生のまま出るため、スマートフォン幅では一段小さくする (→ docs/ui.md「UI 全般」)。 */
const compactListRowClass = `relative flex min-h-16 w-full items-center gap-3.5 py-2.5 text-left transition-colors hover:bg-muted/50 sm:min-h-18 sm:gap-4 sm:py-3 ${browseGutterClass}`;

/** 一覧の上の段に置く切り替え (リストとタイル・検索の範囲) の1つずつ。高さは並び順のドロップダウンと揃える。 */
export const browseToggleItemClass =
	'h-14 bg-background text-muted-foreground data-[state=on]:bg-primary/10 data-[state=on]:text-primary';

/** タイルの並べ。1枚の幅が 170〜230px ほどに収まるよう、幅に応じて列を増やす (→ docs/ui.md「UI 全般」)。 */
const tileListClass = `grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 2xl:grid-cols-8 ${browseGutterClass}`;

// 見えている所だけを描く一覧は、タイルを1段ずつ描くので、1段の枚数を画面の側で決める。
// 段の列もこの数で引くので (`browseVirtualLineStyle`)、CSS と食い違って段の中で折り返すことはない。
// 幅は上の `grid-cols-*` と同じ (Tailwind の既定のブレークポイント) にし、ほかの一覧とタイルの大きさを揃える。
const tileColumnQueries: [MediaQuery, number][] = [
	[new MediaQuery('min-width: 96rem'), 8],
	[new MediaQuery('min-width: 80rem'), 6],
	[new MediaQuery('min-width: 64rem'), 5],
	[new MediaQuery('min-width: 48rem'), 4],
	[new MediaQuery('min-width: 40rem'), 3]
];

/** 見えている所だけを描く一覧で、1つの段に並べる行の数。リストは1行ずつ、タイルは1段の枚数。 */
export function browseRowsPerLine(): number {
	if (!browseLayout.tile) return 1;
	return tileColumnQueries.find(([query]) => query.current)?.[1] ?? 2;
}

/** 見えている所だけを描く一覧の外枠。行とタイルの1段は、この中に1つずつ重ねて置かれる。 */
export function browseVirtualListClass(): string {
	return browseLayout.tile ? browseGutterClass : listClass;
}

/** 見えている所だけを描く一覧の1段。`last:` が効かない (1段ずつ別の要素に入る) ので、最後かを渡す。 */
export function browseVirtualLineClass(last: boolean): string {
	if (browseLayout.tile) return last ? 'grid gap-3' : 'grid gap-3 pb-3';
	return last ? '' : listItemBorderClass;
}

/** 見えている所だけを描く一覧の1段の列。タイルは `browseRowsPerLine()` の数だけ等分に並べる。 */
export function browseVirtualLineStyle(perLine: number): string | undefined {
	return browseLayout.tile
		? `grid-template-columns: repeat(${perLine}, minmax(0, 1fr))`
		: undefined;
}

/**
 * タイルの右上の隅。新規タブの印と、画像を開いている間の印を置く。
 * 枠いっぱいに広げたタイトルのリンクの上に重なるので、押したときはリンクに届かせる。
 */
export const tileCornerClass = 'pointer-events-none absolute top-2 right-2';

/**
 * タイル1枚。上に 16:9 の帯 (リンクは画像、ほかはアイコンか縮小画像)、下に左寄せの文字を置くカード (→ docs/ui.md「UI 全般」)。
 * 行と同じく要素の全体を押せる。
 */
const tileClass =
	'relative flex h-full w-full flex-col overflow-hidden rounded-lg border bg-background text-left transition-colors hover:bg-muted/50';

/** タイルの上の帯。 */
export const tileBandClass =
	'flex aspect-video w-full shrink-0 items-center justify-center overflow-hidden bg-muted';

/** 閲覧側の一覧の `<ul>`。 */
export function browseListClass(): string {
	return browseLayout.tile ? tileListClass : listClass;
}

/** 閲覧側の一覧の `<li>`。タイルは格子の1マスで、長い名前に押し広げられないよう幅の下限を外す。 */
export function browseItemClass(): string {
	return browseLayout.tile ? 'min-w-0' : listItemClass;
}

/** 閲覧側の一覧の行・タイル。`compact` はフォルダー一覧の行で、タイルには効かない。 */
export function browseRowClass(compact = false): string {
	if (browseLayout.tile) return tileClass;
	return compact ? compactListRowClass : listRowClass;
}

/**
 * タイトルを包むリンク・ボタン。押せる範囲を `::after` で行・タイルの枠いっぱいに広げ、フォーカスの枠もそこに出す。
 */
export function browseRowPressClass(): string {
	const pressClass =
		'block w-full outline-none after:absolute after:inset-0 focus-visible:after:outline-2 focus-visible:after:-outline-offset-2 focus-visible:after:outline-ring';
	return browseLayout.tile ? `${pressClass} after:rounded-lg` : pressClass;
}

/** 行いっぱいに広げたリンクの上に重ねて、別に押せるようにする要素。 */
export const browseRowAboveClass = 'relative z-10';

/**
 * 閲覧の一覧の行の右端に置く ⋮ メニューの入れ物。行いっぱいのリンクの上に重ねる。
 * タイルでは、新規タブの印と同じ右上の隅に置く (印は出さない)。
 */
export function rowMenuSlotClass(): string {
	return browseLayout.tile ? 'absolute top-1 right-1 z-10' : browseRowAboveClass;
}

/** 行の文字 (タイトルと2段目) を包む要素。 */
export function browseRowTextClass(): string {
	return browseLayout.tile
		? 'flex w-full min-w-0 flex-col gap-1 px-3 pt-2.5 pb-3'
		: 'min-w-0 flex-1';
}

/** 行の1段目。タイルは幅が狭いので一段小さくし、2行を超えた分は切る。区切りの無いファイル名も折り返す。 */
export function browseRowTitleClass(compact = false): string {
	if (browseLayout.tile) return 'line-clamp-2 text-base leading-6 wrap-anywhere';
	return compact ? compactListRowTitleClass : listRowTitleClass;
}

/**
 * 行の2段目。タイルでは2行まで出し、超えた分は切る。段との間はタイルの文字の段の `gap` が受け持つ。
 * `parts` は、中の項目がそれぞれ縮む並び (場所のボタンを含む段) で、1行に収める。段ごと切ると、ボタンの押せる範囲の広がりまで切れるため。
 */
export function browseRowSubtitleClass(parts = false): string {
	if (!browseLayout.tile) return listRowSubtitleClass;
	// display は `flex`・`line-clamp-2` (-webkit-box) がそれぞれ決めるので、`block` を並べない (後に来た方が勝ってしまう)。
	const tileSubtitleClass = 'text-xs text-muted-foreground sm:text-sm';
	return parts
		? `${tileSubtitleClass} flex gap-1`
		: `${tileSubtitleClass} line-clamp-2 wrap-anywhere`;
}
