import { client } from '$lib/api/client';
import { fetchOrNull } from '$lib/api/load';
import { searchScopeFromQuery } from '$lib/search-scope';
import type { PageLoad } from './$types';

// 語と範囲は URL に持つ (→ docs/search.md)。打ちながら探すので、取れなくても画面ごとエラーにはせず、
// 結果の場所に理由を出す。
export const load: PageLoad = async ({ parent, url }) => {
	await parent();

	const q = url.searchParams.get('q') ?? '';
	const scope = searchScopeFromQuery(url);
	const all = scope !== null && url.searchParams.get('all') === '1';
	// 語が空でも範囲があれば問い合わせる。切り替えに出す範囲の名前が要るため。
	if (q.trim() === '' && scope === null) {
		return { q, scope, all, scopeTitle: null, result: null, failed: false };
	}
	const response = await fetchOrNull(
		client.GET('/api/v1/search', {
			params: {
				query: {
					q,
					...(scope ? { within: Number(scope.within), path: scope.path ?? '', all } : {})
				}
			}
		})
	);
	return {
		q,
		scope,
		all,
		scopeTitle: response?.scopeTitle ?? null,
		result: q.trim() === '' ? null : response,
		failed: response === null
	};
};
