// ページ内で開いているもの (画像・ファイルのビューアー) を、URL の `preview` に持つ決まり
// (→ docs/ui.md「PDF・動画・テキストのビューアー」)。履歴の書き方は $lib/preview-url.svelte.ts。

import type { ReadonlyURL } from '$app/state';
import { queryParams, withQuery } from '#lib/href.ts';

/** 開いているものの key を持つ URL クエリのキー。アーカイブの画面の URL では、軸の絞り込みとして読まない。 */
export const PREVIEW_QUERY = 'preview';

/**
 * 今の URL に開いているべき key。shallow routing は `page.url` を変えないので `page.state` を先に読み
 * (`null` は「開いていない」)、無ければ (読み込み直し・共有されたリンク) URL から読む。
 */
export function wantedPreviewKey(state: App.PageState, url: ReadonlyURL): string | null {
	if (state.preview !== undefined) return state.preview;
	return url.searchParams.get(PREVIEW_QUERY);
}

/** `pathname` と `search` (今のブラウザーの URL) の `preview` だけを `key` に替えた href。`null` なら外す。ほかのクエリは残す。 */
export function previewHref(pathname: string, search: string, key: string | null): string {
	const params = queryParams(search);
	delete params[PREVIEW_QUERY];
	return withQuery(pathname, key === null ? params : { ...params, [PREVIEW_QUERY]: key });
}

/**
 * アーカイブの行の key。手元に全部の行が無いので、読み込み直したときにどの区切りを取ればよいか分かるよう、
 * 並びの位置を前に付ける。位置は目安で、見分けるのはファイルの番号。
 */
export function archivePreviewKey(index: number, itemId: number): string {
	return `${index}-${itemId}`;
}

/** `archivePreviewKey` の形でなければ `undefined`。 */
export function parseArchivePreviewKey(key: string): { index: number; itemId: number } | undefined {
	const match = /^(\d+)-(\d+)$/.exec(key);
	if (!match) return undefined;
	return { index: Number(match[1]), itemId: Number(match[2]) };
}
