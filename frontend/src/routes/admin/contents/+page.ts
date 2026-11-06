import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import * as m from '$lib/paraglide/messages.js';
import { fetchSharedFolderPresence } from '$lib/shared-folders';
import type { PageLoad } from './$types';

// この画面はログイン済みなら `role` を問わず開ける (→ docs/access.md「管理画面の一覧が `user` に見えること」)。
// 未ログインの弾き出しはルートの `+layout.ts` が行う。
// `parent()` を待ってから fetch するのは、リダイレクト判定より先にリクエストが飛ぶのを
// 避けるため。
export const load: PageLoad = async ({ parent, url }) => {
	await parent();

	const [contents, hasSharedFolders] = await Promise.all([
		fetchOrError(client.GET('/api/v1/admin/contents'), m.contents_fetch_failed(), {
			loginRedirectFrom: url
		}),
		fetchSharedFolderPresence()
	]);

	return { contents, hasSharedFolders };
};
