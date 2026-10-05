import { error } from '@sveltejs/kit';
import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// アーカイブの軸定義・値の辞書・表示タイトルの編集画面。読み取り系API
// (list_axes/list_axis_values) と値の辞書の保存は user にも開いており、タブも user に出す。
// 軸のCRUDと、表示タイトルを変える保存 (PUT /contents/{id}) だけがサーバー側403で弾かれる
// (→ docs/access.md「ロールと操作」)。
//
// 対象のコンテンツは [id]/+layout.ts が一覧から探して渡す。
export const load: PageLoad = async ({ parent, url }) => {
	const { content, contentId } = await parent();
	if (content.type !== 'archive') {
		error(404, m.error_not_found());
	}

	const axes = await fetchOrError(
		client.GET('/api/v1/contents/{id}/axes', { params: { path: { id: contentId } } }),
		m.archive_axes_fetch_failed(),
		{ loginRedirectFrom: url }
	);

	return { axes };
};
