import { resolve } from '$app/paths';
import { withQuery } from '$lib/href';

/** 検索の範囲のクエリ (→ docs/search.md「範囲」)。`within` はグループ・フォルダ・アーカイブの id。 */
export type SearchScopeQuery = { within: string; path?: string };

/**
 * 今開いている閲覧ページを、検索の範囲にしたもの。範囲にならない画面 (ホームなど) は `null`。
 * リンクの一覧のページは、その一覧を入れているフォルダの階層かアーカイブにする。
 */
export function searchScopeOf(
	routeId: string | null,
	params: Record<string, string>,
	url: URL
): SearchScopeQuery | null {
	switch (routeId) {
		case '/groups/[id]':
		case '/archives/[id]':
			return { within: params.id };
		case '/folders/[id]':
			return withPath(params.id, url.searchParams.get('path') ?? '');
		case '/links/[id]': {
			if (url.searchParams.has('item')) return { within: params.id };
			const path = url.searchParams.get('path');
			if (path === null) return null;
			return withPath(params.id, path.split('/').slice(0, -1).join('/'));
		}
		default:
			return null;
	}
}

/** 検索の画面の URL に載っている範囲。 */
export function searchScopeFromQuery(url: URL): SearchScopeQuery | null {
	const within = url.searchParams.get('within');
	if (within === null) return null;
	return withPath(within, url.searchParams.get('path') ?? '');
}

function withPath(within: string, path: string): SearchScopeQuery {
	return path === '' ? { within } : { within, path };
}

/** 検索の画面への href。`all` は、範囲を持ったまま全体を探すときに立てる。 */
export function searchHref(q: string, scope: SearchScopeQuery | null, all = false): string {
	return withQuery(resolve('/search'), {
		...(q === '' ? {} : { q }),
		...(scope ?? {}),
		...(scope && all ? { all: '1' } : {})
	});
}
