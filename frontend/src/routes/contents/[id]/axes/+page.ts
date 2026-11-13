import { error } from '@sveltejs/kit';
import { client } from '#lib/api/client.ts';
import { fetchOrError } from '#lib/api/load.ts';
import * as m from '#lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// 分類 (軸)・値の辞書・表示タイトルの編集画面。書き換えられない人は [id]/+layout.ts が閲覧の画面へ移すので、
// ここに来るのは書き換えられる人だけ (→ docs/access.md「ロールと操作」)。
//
// 対象のコンテンツは [id]/+layout.ts が一覧から探して渡す。
export const load: PageLoad = async ({ parent, url }) => {
	const { content, contentId } = await parent();
	if (!content.listView) {
		error(404, m.error_not_found());
	}

	const axes = await fetchOrError(
		client.GET('/api/v1/contents/{id}/axes', { params: { path: { id: contentId } } }),
		m.archive_axes_fetch_failed(),
		{ loginRedirectFrom: url }
	);

	return { axes };
};
