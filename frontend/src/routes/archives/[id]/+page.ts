import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import { parseArchiveSort } from '$lib/archive-sort';
import { LIST_FILTER_QUERY } from '$lib/list-filter.svelte';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// 閲覧レベルが足りない (401) ときだけ、戻り先付きで `/login` へ誘導する (→ docs/access.md「匿名閲覧の受け口」)。
export const load: PageLoad = async ({ parent, params, url }) => {
	await parent();

	const contentId = Number(params.id);

	// クエリキーは軸名(`?科目=国語`)。軸名に一致しないキーはサーバー側が無視するため、
	// ここで検証せずそのまま転送する(→ docs/archive.md「エンドポイント一覧」、共有されたリンクを壊さないため)。
	// `sort` は並び順のクエリなので filters とは別に持つ(→ docs/archive.md「エンドポイント一覧」)。
	// ページ内の絞り込みの語は画面だけのもので、サーバーには送らない (→ docs/ui.md「一覧の絞り込み」)。
	const filters = Object.fromEntries(
		[...url.searchParams].filter(([key]) => key !== LIST_FILTER_QUERY)
	);
	const sort = parseArchiveSort(url.searchParams.get('sort'));

	// utoipaが動的クエリ(軸名がキー)を型付けできないため、生成された型ではこの
	// エンドポイントの`query`が`never`になっている(→ schema.d.tsのコメント)。実行時は
	// openapi-fetchがオブジェクトをそのままURLへ渡すため、型だけキャストして渡す
	// (admin/contents/+page.svelteのFormData bodyキャストと同じ考え方)。
	const requestInit = { params: { path: { id: contentId }, query: filters } } as unknown as {
		params: { path: { id: number } };
	};

	const view = await fetchOrError(
		client.GET('/api/v1/contents/{id}/archive', requestInit),
		m.archive_view_fetch_failed(),
		{ loginRedirectFrom: url }
	);
	return { contentId, view, filters, sort };
};
