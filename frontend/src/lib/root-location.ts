/**
 * コンテンツの登録側の画面に出す場所。起点のフルパスの代わりに「公開できるフォルダー」の
 * 名前を出し、そこから先だけを続ける (例:「教材 / 英検 / 2024」→ docs/folders.md「公開できるフォルダー」)。
 *
 * 画面に出すだけの文字列で、パスとしては使わない。API に渡すのは絶対パスのまま。
 */
import type { components } from '$lib/api/schema';
import { isLoopbackHost } from '$lib/loopback';
import * as m from '$lib/paraglide/messages.js';

type Content = components['schemas']['AdminContentResponse'];

/** 階層の名前の区切り。コンテンツの親グループを並べるときも同じ見た目にする。 */
export const LOCATION_SEPARATOR = ' / ';

/** 名前と、そこから先の階層 (`/` 区切り、フォルダーそのものなら空) をつなぐ。 */
export function rootLocationLabel(rootName: string, pathInRoot: string): string {
	return [rootName, ...pathInRoot.split('/').filter((segment) => segment !== '')].join(
		LOCATION_SEPARATOR
	);
}

/** 階層の名前を並べる。ピッカーのパンくずのように、階層ごとの名前が手元にあるとき用。 */
export function joinLocationLabels(labels: string[]): string {
	return labels.join(LOCATION_SEPARATOR);
}

/**
 * 「公開できるフォルダー」を追加できる場所か。サーバーの PC から開いたときだけ
 * (→ docs/folders.md「公開できるフォルダー」)。画面の出し分けにだけ使い、実際の可否はサーバーが決める。
 */
export function canAddRoots(hostname: string): boolean {
	return isLoopbackHost(hostname);
}

/**
 * コンテンツの場所 (`path`) の出し方。
 *
 * - 登録中の公開できるフォルダーの中: 「名前 / その先」
 * - 公開できるフォルダーの登録を削除した後: 管理者にはフルパス、
 *   それ以外には「(存在しない公開フォルダー) / その先」。起点のパスは見せない
 * - どの公開できるフォルダーの記録にも含まれない: 管理者にはフルパス、
 *   それ以外には「(存在しない公開フォルダー)」だけ (どこまでが起点か分からないため)
 */
export function contentPathLabel(content: Content, canSeeFullPath: boolean): string | null {
	if (content.path == null) return null;
	if (content.rootName != null && !content.rootDeleted && content.pathInRoot != null) {
		return rootLocationLabel(content.rootName, content.pathInRoot);
	}
	if (canSeeFullPath) return content.path;
	return rootLocationLabel(m.contents_path_root_deleted(), content.pathInRoot ?? '');
}
