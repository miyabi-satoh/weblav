import { error } from '@sveltejs/kit';
import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// 1件を取る API は無いので、一覧から探す (共有の編集ページと同じ)。
// admin 限定であることは API と上の `+layout.ts` が確かめる。
// 自分自身は開かない。名前とパスワードはアカウントのページで変え、降格と削除はほかの管理者がする
// (→ docs/access.md「ロールと操作」)。サーバーも自分への操作を断る。
// 返す名前は `user` にしない。親の `user` (ログイン中の自分) を上書きしてしまうため。
export const load: PageLoad = async ({ parent, params, url }) => {
	const { user } = await parent();
	const id = Number(params.id);
	const users = await fetchOrError(
		client.GET('/api/v1/admin/users'),
		m.admin_users_fetch_failed(),
		{
			loginRedirectFrom: url
		}
	);
	const target = users.find((candidate) => candidate.id === id);
	if (!target || target.id === user?.id) {
		error(404, m.error_not_found());
	}
	return { target };
};
