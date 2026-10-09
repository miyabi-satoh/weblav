import { client } from '$lib/api/client';
import { fetchOrNull } from '$lib/api/load';
import { searchScopeFromQuery, type SearchScopeQuery } from '$lib/search-scope';
import type { PageLoad } from './$types';

// 語と範囲は URL に持つ (→ docs/search.md)。打ちながら探すので、取れなくても画面ごとエラーにはせず、
// 結果の場所に理由を出す。
export const load: PageLoad = async ({ parent, url }) => {
	await parent();

	const q = url.searchParams.get('q') ?? '';
	let scope = searchScopeFromQuery(url);
	const all = scope !== null && url.searchParams.get('all') === '1';
	// 語が空でも範囲があれば問い合わせる。切り替えに出す範囲の名前が要るため。
	if (q.trim() === '' && scope === null) {
		return { q, scope, all, scopeTitle: null, result: null, failed: false };
	}
	let response = await fetchOrNull(searchRequest(q, scope, all));
	// 範囲の場所が開けない (消えた・ログインが切れた) ときは、範囲を外して全体を探す。
	// 範囲を持ったままだと切り替えも出せず、どの語でも失敗し続けるため。
	if (response === null && scope !== null) {
		scope = null;
		response = q.trim() === '' ? null : await fetchOrNull(searchRequest(q, null, false));
	}
	return {
		q,
		scope,
		all: scope !== null && all,
		scopeTitle: response?.scopeTitle ?? null,
		result: q.trim() === '' ? null : response,
		failed: q.trim() !== '' && response === null
	};
};

function searchRequest(q: string, scope: SearchScopeQuery | null, all: boolean) {
	return client.GET('/api/v1/search', {
		params: {
			query: {
				q,
				...(scope ? { within: Number(scope.within), path: scope.path ?? '', all } : {})
			}
		}
	});
}
