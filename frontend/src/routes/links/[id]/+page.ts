import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import type { LinksFileTarget } from '$lib/links-file';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// 閲覧レベルが足りない (401) ときだけ、戻り先付きで `/login` へ誘導する (→ docs/access.md「匿名閲覧の受け口」)。
export const load: PageLoad = async ({ parent, params, url }) => {
	await parent();

	const contentId = Number(params.id);
	const item = url.searchParams.get('item');
	const target: LinksFileTarget = {
		path: url.searchParams.get('path') ?? undefined,
		item: item === null ? undefined : Number(item)
	};

	const linksFile = await fetchOrError(
		client.GET('/api/v1/contents/{id}/links', {
			params: { path: { id: contentId }, query: target }
		}),
		m.links_file_fetch_failed(),
		{ loginRedirectFrom: url }
	);
	// 開いた一覧の画面の並び順・絞り込み。パンくずの戻り先に付ける (→ `linksFileHref`)。
	const listQuery = Object.fromEntries(new URLSearchParams(url.searchParams.get('from') ?? ''));
	return { contentId, target, listQuery, linksFile };
};
