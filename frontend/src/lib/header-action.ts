import { buttonVariants } from '$lib/components/ui/button';
import { cn } from '$lib/utils';

/**
 * ヘッダーの操作で共有する class。枠を付けず補助色で置く (→ docs/ui.md「UI 全般」)。
 */

/** アイコンだけの操作 (言語・テーマの切り替え)。 */
export const headerIconActionClass = cn(
	buttonVariants({ variant: 'ghost', size: 'icon' }),
	'text-muted-foreground'
);

/**
 * 文字の操作 (ユーザーメニュー・ログイン)。高さは `headerIconActionClass` に揃える。
 * ヘッダーの右端に置くので、右の padding を打ち消して文字の端を余白の線に揃える。
 */
export const headerTextActionClass = cn(
	buttonVariants({ variant: 'ghost', size: 'sm' }),
	'-mr-2.5 h-8 text-muted-foreground'
);
