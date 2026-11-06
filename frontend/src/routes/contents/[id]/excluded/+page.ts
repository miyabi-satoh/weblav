import { error } from '@sveltejs/kit';
import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import { LIST_FILTER_QUERY } from '$lib/list-filter';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// ライブラリの対象外のタブ (→ docs/ui.md「対象外のタブ」)。
export const load: PageLoad = async ({ parent, url }) => {
	const { content, contentId } = await parent();
	if (!content.listView) {
		error(404, m.error_not_found());
	}
	const nameFilter = url.searchParams.get(LIST_FILTER_QUERY) ?? '';
	const view = await fetchOrError(
		client.GET('/api/v1/contents/{id}/excluded-items', {
			params: { path: { id: contentId }, query: { filter: nameFilter } }
		}),
		m.archive_view_fetch_failed(),
		{ loginRedirectFrom: url }
	);
	return { view, nameFilter };
};
