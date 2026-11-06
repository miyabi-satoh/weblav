/**
 * 場所を選ぶ窓 (サーバーのフォルダーを選ぶ窓・共有の中の置き先を選ぶ窓) で共有する class。
 * 2つの窓で、パンくずと行の見た目と押せる大きさを揃える。
 */

/** パンくずの段。実際の高さを 44px にする (→ docs/ui.md「UI 全般」)。 */
export const pickerNavClass =
	'flex min-h-11 flex-wrap items-center gap-x-2 border-b border-divider bg-muted/40 px-4';

/** パンくずの、押して戻る段。名前が1文字でも 44px 四方を押せるようにする。 */
export const pickerCrumbButtonClass =
	'inline-flex h-11 min-w-11 items-center justify-center text-xs text-primary underline underline-offset-4';

/** 一覧の1行。選べない行 (`disabled`) は押せないことが見て分かるようにする。 */
export const pickerRowClass =
	'flex h-11 w-full items-center gap-3 border-b border-divider px-4 text-left hover:bg-muted/60 disabled:pointer-events-none disabled:text-muted-foreground';
