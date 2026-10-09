import { resolve } from '$app/paths';
import { withQuery } from '$lib/href';

/** どの一覧のファイルか。フォルダーの中は `path`、アーカイブのアイテムは `item`、`file` コンテンツは指さない。 */
export type LinksFileTarget = { path?: string; item?: number };

/**
 * 一覧のファイルを開く画面 (`/links/[id]`) への href。
 * `listQuery` は開いた一覧の画面の並び順・絞り込み。パンくずで戻ったときに選び直さずに済むよう、`from` に入れて持ち回す。
 */
export function linksFileHref(
	contentId: number,
	target: LinksFileTarget = {},
	listQuery: Record<string, string> = {}
): string {
	const from = new URLSearchParams(listQuery).toString();
	return withQuery(resolve('/links/[id]', { id: String(contentId) }), {
		...(target.path ? { path: target.path } : {}),
		...(target.item !== undefined ? { item: String(target.item) } : {}),
		...(from ? { from } : {})
	});
}

/** 一覧の行のアイコン。中身がリンクの並びであることを、リンクと同じ絵で示す。 */
export { default as LinksFileIcon } from '@lucide/svelte/icons/link';
