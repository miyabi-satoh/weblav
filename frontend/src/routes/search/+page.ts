import { client } from '$lib/api/client';
import { fetchOrNull } from '$lib/api/load';
import type { PageLoad } from './$types';

// 語は URL の `q` に持つ (→ docs/search.md)。打ちながら探すので、取れなくても画面ごとエラーにはせず、
// 結果の場所に理由を出す。
export const load: PageLoad = async ({ parent, url }) => {
	await parent();

	const q = url.searchParams.get('q') ?? '';
	if (q.trim() === '') return { q, result: null, failed: false };
	const result = await fetchOrNull(client.GET('/api/v1/search', { params: { query: { q } } }));
	return { q, result, failed: result === null };
};
