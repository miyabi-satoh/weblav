import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import { archiveListInit, axisFilters } from '$lib/archive-list-request';
import { parseArchiveSort } from '$lib/archive-sort';
import { LIST_FILTER_QUERY } from '$lib/list-filter';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// 閲覧レベルが足りない (401) ときだけ、戻り先付きで `/login` へ誘導する (→ docs/access.md「匿名閲覧の受け口」)。
export const load: PageLoad = async ({ parent, params, url }) => {
	await parent();

	const contentId = Number(params.id);

	// 画面の URL のクエリキーは軸名(`?科目=国語`)。軸名に一致しないキーはサーバー側が無視するため、
	// ここで検証せずそのまま転送する(→ docs/archive.md「エンドポイント一覧」、共有されたリンクを壊さないため)。
	// 名前の語はサーバーで照らす (→ docs/archive.md「一覧の API を区切って返す」)。語を打ち直したときは、
	// 読み込みを走らせずに画面が取り直す (→ +page.svelte)。
	const filters = axisFilters(url);
	const sort = parseArchiveSort(url.searchParams.get('sort'));
	const nameFilter = url.searchParams.get(LIST_FILTER_QUERY) ?? '';

	const requestInit = archiveListInit(contentId, filters, sort, nameFilter);

	const view = await fetchOrError(
		client.GET('/api/v1/contents/{id}/archive', requestInit),
		m.archive_view_fetch_failed(),
		{ loginRedirectFrom: url }
	);
	return { contentId, view, filters, sort, nameFilter };
};
