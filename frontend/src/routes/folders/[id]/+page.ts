import { client } from '$lib/api/client';
import { fetchOrError, fetchOrNull } from '$lib/api/load';
import { parseBrowseSort } from '$lib/browse-sort';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// 閲覧レベルが足りない (401) ときだけ、戻り先付きで `/login` へ誘導する (→ docs/access.md「匿名閲覧の受け口」)。
export const load: PageLoad = async ({ parent, params, url }) => {
	await parent();

	const contentId = Number(params.id);
	const path = url.searchParams.get('path') ?? '';
	// 並び順はホーム/グループと同じ扱い (→ docs/ui.md「ホーム・グループ・フォルダー・アーカイブの並び順」)。
	const sort = parseBrowseSort(url.searchParams.get('sort'));

	const browse = await fetchOrError(
		client.GET('/api/v1/contents/{id}/browse', {
			params: {
				path: { id: contentId },
				query: { path, sort }
			}
		}),
		m.folder_browse_fetch_failed(),
		{ loginRedirectFrom: url }
	);
	// 共有の中で操作できる場所なら、移動・コピーの置き先の候補を取る (→ docs/ui.md「共有の中の画面」)。
	// 取れなくても一覧は出し、コピーを出さないだけにする。
	const shares = browse.share ? ((await fetchOrNull(client.GET('/api/v1/shares'))) ?? []) : [];
	return { contentId, browse, sort, shares };
};
