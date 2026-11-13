import { error } from '@sveltejs/kit';
import { client } from '#lib/api/client.ts';
import { fetchOrError } from '#lib/api/load.ts';
import { archiveListInit, axisFilters } from '#lib/archive-list-request.ts';
import { parseArchiveSort } from '#lib/archive-sort.ts';
import { LIST_FILTER_QUERY } from '#lib/list-filter.ts';
import * as m from '#lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// 一覧の表示を使うフォルダーの、ファイルのタブ。閲覧の画面 (`/archives/[id]`) と同じ絞り込みと並べ替えで並べる
// (→ docs/ui.md「アーカイブのファイルのタブ」)。クエリの扱いは閲覧の画面の +page.ts と同じ。
export const load: PageLoad = async ({ parent, url }) => {
	const { content, contentId } = await parent();
	if (!content.listView) {
		error(404, m.error_not_found());
	}

	const filters = axisFilters(url);
	const sort = parseArchiveSort(url.searchParams.get('sort'));
	const nameFilter = url.searchParams.get(LIST_FILTER_QUERY) ?? '';

	const requestInit = archiveListInit(contentId, filters, sort, nameFilter);
	const view = await fetchOrError(
		client.GET('/api/v1/contents/{id}/manage-archive', requestInit),
		m.archive_view_fetch_failed(),
		{ loginRedirectFrom: url }
	);
	return { view, filters, sort, nameFilter };
};
