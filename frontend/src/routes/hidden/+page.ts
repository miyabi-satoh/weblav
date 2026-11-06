import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import { parseBrowseSort } from '$lib/browse-sort';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// 非表示のコンテンツは閲覧の場のどこにも並ばないので、ここに集めて開けるようにする。
// 並び順はホームやグループと同じ扱い (→ docs/ui.md「ホーム・グループ・フォルダー・アーカイブの並び順」)。
export const load: PageLoad = async ({ parent, url }) => {
	await parent();
	const sort = parseBrowseSort(url.searchParams.get('sort'));
	const hidden = await fetchOrError(
		client.GET('/api/v1/contents/hidden', { params: { query: { sort } } }),
		m.hidden_fetch_failed(),
		{ loginRedirectFrom: url }
	);
	return { hidden, sort };
};
