/**
 * 画面を切り替えるタブ (管理画面の上段と、コンテンツの編集ページ) の見た目。
 * 文字だけにして、選択中を差し色の下線で示す。管理画面の上段は、スマートフォン幅だけ絵を使う
 * (`adminNavTabListClass`、→ docs/ui.md「UI 全般」)。
 * 編集ページの中のタブは地の色のままにし、白い帯の上段と2段に重なって見えないようにする。
 */
export const navTabListClass = 'flex gap-6 border-b px-4 md:px-6';

/**
 * 管理画面の上段のタブの列。スマートフォン幅では選んでいないタブを絵だけにするので、
 * 間隔を詰める (押せる範囲は各タブの幅で取る)。
 * それでも収まらない狭い端末では、ページごと横に広がらないよう列の中で横に送る。
 */
export const adminNavTabListClass =
	'flex gap-2 overflow-x-auto border-b bg-background px-4 sm:gap-6 md:px-6';

export function navTabClass(current: boolean): string {
	return current
		? 'flex h-11 items-center border-b-2 border-primary text-sm text-foreground'
		: 'flex h-11 items-center border-b-2 border-transparent text-sm text-sub-foreground hover:text-foreground';
}
