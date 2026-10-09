import { browseLayout } from '$lib/browse-layout.svelte';

/**
 * 閲覧側の一覧 (トップ・グループ・アーカイブ・フォルダ) で共有する class。
 * 1列リストとタイルの2つの並べ方を持ち、`browse*Class()` が端末で選んだ方を返す (→ $lib/browse-layout.svelte.ts)。
 * 行の中身は `list-row-icon.svelte` / `list-row-glyph.svelte`。行そのもの (`<a>` / `<button>`) は
 * 各画面に置く。`resolve()` や `rel="external"` を eslint (svelte/no-navigation-without-resolve) が
 * 静的に見られるようにするため。
 * 管理画面「公開できるフォルダ」(`admin/roots`) は `listClass`/`listItemClass` (枠) だけを使い、
 * 行本体はクリック不可・ホバーなしの一覧なので別に持つ。
 */
export const listClass = 'border-y bg-background';

/** 行の区切り。読み物のページの枠付きリスト (`page-layout.ts`) も同じ引き方にする。 */
export const listItemClass = 'border-b border-divider last:border-b-0';

/** 閲覧側の本文の左右の余白。行と、その外に置く見出し・文言で揃える。 */
export const browseGutterClass = 'px-4 sm:px-6';

const listRowClass = `flex min-h-18 w-full items-center gap-4 py-3 text-left transition-colors hover:bg-muted/50 ${browseGutterClass}`;

/** 行の1段目 (タイトル・名前)。 */
const listRowTitleClass = 'block text-lg leading-6';

/** フォルダ一覧用のタイトル。行と同じく、スマートフォン幅では一段小さくする (→ docs/ui.md「UI 全般」)。 */
const compactListRowTitleClass = 'block truncate text-base leading-6 sm:text-lg';

/** 行の2段目 (ホーム・グループの説明、アーカイブの軸の値、フォルダのサイズと日時)。 */
const listRowSubtitleClass = 'mt-0.5 block text-xs text-muted-foreground sm:text-sm';

/** フォルダ一覧用。名前が生のまま出るため、スマートフォン幅では一段小さくする (→ docs/ui.md「UI 全般」)。 */
const compactListRowClass = `flex min-h-16 w-full items-center gap-3.5 py-2.5 text-left transition-colors hover:bg-muted/50 sm:min-h-18 sm:gap-4 sm:py-3 ${browseGutterClass}`;

/** 一覧の上の段。左に並び順 (と絞り込み)、右端にリストとタイルの切り替えを置く (→ docs/ui.md「UI 全般」)。 */
export const browseControlsClass = 'flex items-start justify-between gap-3';

/** 一覧の上の段に置く切り替え (リストとタイル・検索の範囲) の1つずつ。高さは並び順のドロップダウンと揃える。 */
export const browseToggleItemClass =
	'h-14 bg-background text-muted-foreground data-[state=on]:bg-primary/10 data-[state=on]:text-primary';

/** タイルの並べ。1枚の幅が 170〜230px ほどに収まるよう、幅に応じて列を増やす (→ docs/ui.md「UI 全般」)。 */
const tileListClass = `grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 2xl:grid-cols-8 ${browseGutterClass}`;

/** タイルの右上の隅。新規タブの印と、画像を開いている間の印を置く。 */
export const tileCornerClass = 'absolute top-2 right-2';

/** タイル1枚。行と同じく要素の全体を押せる。`relative` は `tileCornerClass` の印のため。 */
const tileClass =
	'relative flex h-full w-full flex-col items-center gap-2 rounded-lg border bg-background px-3 pt-4 pb-3 text-center transition-colors hover:bg-muted/50';

/**
 * リンクのタイル。ほかのタイルと違い、上に大きい画像を端まで置くカードにする (→ docs/ui.md「リンクのカード」)。
 * 押せる範囲と枠は `tileClass` に揃える。
 */
export const linkTileClass =
	'flex h-full w-full flex-col overflow-hidden rounded-lg border bg-background text-left transition-colors hover:bg-muted/50';

/** 閲覧側の一覧の `<ul>`。 */
export function browseListClass(): string {
	return browseLayout.tile ? tileListClass : listClass;
}

/** 閲覧側の一覧の `<li>`。タイルは格子の1マスで、長い名前に押し広げられないよう幅の下限を外す。 */
export function browseItemClass(): string {
	return browseLayout.tile ? 'min-w-0' : listItemClass;
}

/** 閲覧側の一覧の行・タイル。`compact` はフォルダ一覧の行で、タイルには効かない。 */
export function browseRowClass(compact = false): string {
	if (browseLayout.tile) return tileClass;
	return compact ? compactListRowClass : listRowClass;
}

/** 行の文字 (タイトルと2段目) を包む要素。 */
export function browseRowTextClass(): string {
	return browseLayout.tile ? 'w-full min-w-0' : 'min-w-0 flex-1';
}

/** 行の1段目。タイルは幅が狭いので一段小さくし、2行を超えた分は切る。区切りの無いファイル名も折り返す。 */
export function browseRowTitleClass(compact = false): string {
	if (browseLayout.tile) return 'line-clamp-2 text-base leading-6 wrap-anywhere';
	return compact ? compactListRowTitleClass : listRowTitleClass;
}

/** 行の2段目。タイルでは1行に収め、超えた分は切る。 */
export function browseRowSubtitleClass(): string {
	return browseLayout.tile ? `${listRowSubtitleClass} truncate` : listRowSubtitleClass;
}
