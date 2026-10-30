import { resolve } from '$app/paths';

/**
 * 共有の中の一覧 (`/shares/[id]`) へのリンク。
 * そのルートを置くまでは `resolve('/shares/[id]')` が型で通らないので、一覧のパスに番号を足して組む。
 */
export function shareHref(shareId: number): string {
	return `${resolve('/shares')}/${shareId}`;
}
