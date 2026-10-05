import { error } from '@sveltejs/kit';
import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import { canEditContent } from '$lib/auth';
import * as m from '$lib/paraglide/messages.js';
import type { LayoutLoad } from './$types';

// 単一コンテンツ取得APIが無いため、一覧を取って対象idを探す。
// 一覧そのものも、編集ページが親グループの候補と公開範囲の判定に使う。
export const load: LayoutLoad = async ({ parent, params, url }) => {
	const { user } = await parent();

	const id = Number(params.id);

	const contents = await fetchOrError(
		client.GET('/api/v1/admin/contents'),
		m.contents_fetch_failed(),
		{ loginRedirectFrom: url }
	);
	const content = contents.find((c) => c.id === id);
	if (!content) {
		error(404, m.error_not_found());
	}

	// 他人のものを開いた `user` には見るだけにする (→ docs/ui.md「UI 全般」)。タブのどのページも使う。
	return { contents, content, contentId: id, editable: canEditContent(user, content) };
};
