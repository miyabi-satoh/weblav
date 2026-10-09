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

/**
 * 切り替えに出す範囲の名前の、フォルダの名前の後ろに続ける階層 (` / 2025 / 2`)。フォルダの中の階層でなければ空。
 * 同じ名前の階層 (`2024` など) を見分けられるよう最後の2段までを出し、それより上は `…` に詰める。
 * 画面は、幅が足りなければフォルダの名前だけを切り詰め、これは切り詰めない。
 */
export function searchScopeLevel(path: string | undefined): string {
	if (!path) return '';
	const parts = path.split('/');
	const shown = parts.length > 2 ? ['…', ...parts.slice(-2)] : parts;
	return shown.map((part) => ` / ${part}`).join('');
}

/** 検索の画面の URL に載っている範囲。 */
export function searchScopeFromQuery(url: URL): SearchScopeQuery | null {
	const within = url.searchParams.get('within');
	// 手で書き換えた URL などで id でなければ、範囲を持たない。
	if (within === null || !/^\d+$/.test(within)) return null;
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
