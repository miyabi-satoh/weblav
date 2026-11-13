import type { ReadonlyURL } from '$app/state';
import { resolve } from '$app/paths';
import { withQuery } from '#lib/href.ts';

/**
 * 検索の範囲のクエリ (→ docs/search.md「範囲」)。`within` はグループ・フォルダー・アーカイブの id、`share` は共有の id。
 * `path` はフォルダーか共有の中の階層。
 */
export type SearchScopeQuery = { within: string; path?: string } | { share: string; path?: string };

/**
 * 今開いている閲覧ページを、検索の範囲にしたもの。範囲にならない画面 (ホームなど) は `null`。
 * リンクの一覧のページは、その一覧を入れているフォルダーの階層かアーカイブにする。
 */
export function searchScopeOf(
	routeId: string | null,
	params: Record<string, string>,
	url: ReadonlyURL
): SearchScopeQuery | null {
	switch (routeId) {
		case '/groups/[id]':
		case '/archives/[id]':
			return { within: params.id };
		case '/folders/[id]':
			return withPath(params.id, url.searchParams.get('path') ?? '');
		case '/shares/[id]': {
			const path = url.searchParams.get('path') ?? '';
			return path === '' ? { share: params.id } : { share: params.id, path };
		}
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
 * 切り替えに出す範囲の名前の、フォルダーの名前の後ろに続ける階層。フォルダーの中の階層でなければ、どちらも空。
 * 同じ名前の階層 (`2024` など) を見分けられるよう最後の2段までを出し、それより上は `…` に詰める。
 * 上の段 (`upper`) と最後の段 (`last`) は、画面がスマートフォンの幅で別々の上限で切り詰める (→ docs/search.md「範囲」)。
 */
export function searchScopeLevels(path: string | undefined): { upper: string; last: string } {
	if (!path) return { upper: '', last: '' };
	const parts = path.split('/');
	const upper = parts.length > 2 ? ['…', parts[parts.length - 2]] : parts.slice(0, -1);
	return {
		upper: upper.map((part) => ` / ${part}`).join(''),
		last: ` / ${parts[parts.length - 1]}`
	};
}

/** 検索の画面の URL に載っている範囲。 */
export function searchScopeFromQuery(url: ReadonlyURL): SearchScopeQuery | null {
	const path = url.searchParams.get('path') ?? '';
	// 手で書き換えた URL などで id でなければ、範囲を持たない。
	const within = url.searchParams.get('within');
	if (within !== null) return /^\d+$/.test(within) ? withPath(within, path) : null;
	const share = url.searchParams.get('share');
	if (share === null || !/^\d+$/.test(share)) return null;
	return path === '' ? { share } : { share, path };
}

function withPath(within: string, path: string): SearchScopeQuery {
	return path === '' ? { within } : { within, path };
}

/** 検索の画面への href。`all` は、範囲を持ったまま全体を探すときに立てる。 */
export function searchHref(q: string, scope: SearchScopeQuery | null, all = false): string {
	return withQuery(resolve('search'), {
		...(q === '' ? {} : { q }),
		...(scope ?? {}),
		...(scope && all ? { all: '1' } : {})
	});
}
