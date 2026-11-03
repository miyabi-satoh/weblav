import { client } from '$lib/api/client';
import { fetchOrError, fetchOrNull } from '$lib/api/load';
import { parseBrowseSort } from '$lib/browse-sort';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// 共有はログインした人のもの (→ docs/ui.md「共有の中の画面」)。場所はクエリの `path` (共有からの相対パス)。
export const load: PageLoad = async ({ parent, params, url }) => {
	await parent();

	const shareId = Number(params.id);
	const path = url.searchParams.get('path') ?? '';
	const sort = parseBrowseSort(url.searchParams.get('sort'));

	const [browse, shares] = await Promise.all([
		fetchOrError(
			client.GET('/api/v1/shares/{id}/browse', {
				params: { path: { id: shareId }, query: { path, sort } }
			}),
			m.shares_browse_fetch_failed(),
			{ loginRedirectFrom: url }
		),
		// 移動・コピーの置き先の候補。取れなくても一覧は出し、コピーを出さないだけにする。
		fetchOrNull(client.GET('/api/v1/shares'))
	]);
	return { shareId, browse, shares: shares ?? [], sort };
};
