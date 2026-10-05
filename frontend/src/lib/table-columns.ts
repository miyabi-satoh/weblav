/**
 * 管理画面の表で共有する class (→ docs/ui.md「UI 全般」)。
 *
 * 表は横スクロールさせず、狭い幅では優先度の低い列を隠す。どの幅から出すかは
 * 画面ごとに違うので、しきい値だけをここに並べて各画面が選ぶ。
 */

/** 幅が広いときだけ出す列。狭い順に `Sm` → `Md` → `Lg`。見出しとセルの両方に付ける。 */
export const columnFromSmClass = 'hidden sm:table-cell';

export const columnFromMdClass = 'hidden md:table-cell';

export const columnFromLgClass = 'hidden lg:table-cell';

/** 操作 (⋮) の列。幅を固定して、残りをタイトルに渡す。 */
export const actionColumnClass = 'w-12 text-right';

/**
 * 操作 (⋮) メニューのトリガー。見た目は小さいまま、押せる範囲だけを縦 44px に、横は左 8px と
 * 右の画面の端まで (帯の最後のセルの右の余白。幅で変わる → `page-layout.ts`) に広げる (→ docs/ui.md「UI 全般」)。
 */
export const actionMenuTriggerClass =
	'relative inline-flex size-8 items-center justify-center rounded-md text-muted-foreground after:absolute after:-left-2 after:-right-4 after:-inset-y-1.5 md:after:-right-6 hover:bg-muted hover:text-foreground';

/** 短い値 (大きさ・日付・トグル) の列。中身の幅まで縮め、残りをタイトルに回す。 */
export const fitColumnClass = 'w-px whitespace-nowrap';

/**
 * 残りの幅をすべて受け取り、収まらなければ「…」で切る列 (タイトル)。1つの表に1列だけ置く。
 * 例外はアイテムの表のパスで、タイトルと余りを分け合う (→ docs/ui.md「UI 全般」)。
 * `max-w-0` を置くのは、テーブルのセルが中身の幅まで広がろうとするのを止めて
 * `truncate` を効かせるため。
 */
export const truncatingColumnClass = 'max-w-0 truncate';

/**
 * タイトルの次に長い値の列 (パス・URL・親のパンくず)。中身の幅まで縮め、上限を超えたら「…」で切る。
 * セルに付け、値は `cappedValueClass` の要素で包む。セルは中身の幅までしか広がらないので、
 * 上限は包んだ要素の側に付ける。
 */
export const cappedColumnClass = fitColumnClass;

/**
 * 上限は 16rem と画面幅の 15% の小さいほう。16rem で URL の先頭 (ホスト名と最初の階層) が読め、
 * 15% は親と場所の2列を出す lg 幅 (1024px) でもタイトルに表の幅の4割ほどを残すため。
 */
export const cappedValueClass = 'block capped-value-width truncate';

/**
 * 表の行頭のドラッグのつまみ。見た目は 32px のまま、押せる範囲を四方に 6px 広げて 44px にする (→ docs/ui.md「UI 全般」)。
 * `touch-none` はドラッグ中に画面をスクロールさせないため。`select-none` と `touch-callout-none` は、
 * iOS で長押しが範囲選択やメニューにならないようにするため。
 * 大きさと押せる範囲の広がりを変えたら、`layout.css` の `row-link-after-handle`・`row-link-before-handle` も合わせる。
 */
export const dragHandleClass =
	'relative flex size-8 shrink-0 touch-none items-center justify-center text-muted-foreground select-none touch-callout-none after:absolute after:-inset-1.5';
