/**
 * 絞り込みの欄 (文字の入力・選択) で共有する class。値が入って絞り込み中の欄を、同じ色で目立たせる。
 */

/** 絞り込み中の欄の枠と地。 */
export const activeFieldClass = 'border-primary bg-primary/10 dark:bg-primary/10';

/** 欄の中の小さなラベル。絞り込み中は枠と同じ色にする。 */
export function fieldLabelClass(active: boolean): string {
	return active ? 'text-xs text-primary' : 'text-xs text-muted-foreground';
}
