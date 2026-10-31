import { error } from '@sveltejs/kit';
import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// 1件を取る API は無いので、一覧から探す (共有の編集ページと同じ)。最後の管理者かを見るのにも一覧が要る。
// admin 限定であることは API と上の `+layout.ts` が確かめる。
// 返す名前は `user` にしない。親の `user` (ログイン中の自分) を上書きしてしまうため。
export const load: PageLoad = async ({ parent, params, url }) => {
	await parent();
	const id = Number(params.id);
	const users = await fetchOrError(
		client.GET('/api/v1/admin/users'),
		m.admin_users_fetch_failed(),
		{
			loginRedirectFrom: url
		}
	);
	const target = users.find((candidate) => candidate.id === id);
	if (!target) {
		error(404, m.error_not_found());
	}
	return { target, users };
};
