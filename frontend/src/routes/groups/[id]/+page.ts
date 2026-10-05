import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import { parseBrowseSort } from '$lib/browse-sort';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// 閲覧レベルが足りない (401) ときだけ、戻り先付きで `/login` へ誘導する (→ docs/access.md「匿名閲覧の受け口」)。
// group 以外の id も 404 で、エラー表示になる。
export const load: PageLoad = async ({ parent, params, url }) => {
	await parent();

	const contentId = Number(params.id);
	// 並び順はトップと同じ扱い (→ docs/ui.md「ホーム・グループ・フォルダ・アーカイブの並び順」)。
	const sort = parseBrowseSort(url.searchParams.get('sort'));

	const browse = await fetchOrError(
		client.GET('/api/v1/contents/{id}/group', {
			params: { path: { id: contentId }, query: { sort } }
		}),
		m.group_browse_fetch_failed(),
		{ loginRedirectFrom: url }
	);
	return { contentId, browse, sort };
};
