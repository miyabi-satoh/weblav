import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import * as m from '$lib/paraglide/messages.js';
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

/**
 * 「公開できるフォルダー」が1件でもあるか。
 *
 * 1件も無いと folder / archive は登録できないので、種別を選ぶ時点で押せなくする
 * (→ docs/folders.md「公開できるフォルダー」)。0件かどうかは、登録先を選ぶ一覧の上位 (= 登録済みのフォルダー)
 * が空かで分かる。
 *
 * 取れなくても画面ごと落とさない。押せるかどうかのヒントでしかなく、実際の可否は
 * 登録の要求でサーバーが決める。
 */
async function fetchSharedFolderPresence(): Promise<boolean> {
	try {
		const { data } = await client.GET('/api/v1/admin/fs/dirs', {
			params: { query: { path: '' } }
		});
		return (data?.entries.length ?? 0) > 0;
	} catch {
		return false;
	}
}
