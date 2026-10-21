import { client } from '$lib/api/client';
import { fetchOrError, fetchOrNull } from '$lib/api/load';
import { isAdmin } from '$lib/auth';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// 作成者の選択肢 (admin だけに出す → docs/ui.md「UI 全般」)。
export const load: PageLoad = async ({ parent, url }) => {
	const { user, content } = await parent();
	// グループの、AI がリンクを集める設定 (→ docs/ai.md「AI が集めるリンク」)。取れなければ、その欄を出さない。
	const aiLinks =
		content.type === 'group'
			? fetchOrNull(
					client.GET('/api/v1/contents/{id}/ai-links/settings', {
						params: { path: { id: content.id } }
					})
				)
			: null;
	// フォルダーで表示しないにしたもの (→ docs/folders.md「表示しないもの」)。書き換えられない人には 403 なので、出さない。
	const folderHidden =
		content.type === 'folder'
			? fetchOrNull(
					client.GET('/api/v1/contents/{id}/hidden-entries', {
						params: { path: { id: content.id } }
					})
				)
			: null;
	if (!isAdmin(user))
		return { users: [], aiLinks: await aiLinks, folderHidden: await folderHidden };

	const users = await fetchOrError(
		client.GET('/api/v1/admin/users'),
		m.admin_users_fetch_failed(),
		{ loginRedirectFrom: url }
	);
	return { users, aiLinks: await aiLinks, folderHidden: await folderHidden };
};
