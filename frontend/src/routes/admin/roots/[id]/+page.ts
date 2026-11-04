import { error } from '@sveltejs/kit';
import { client } from '$lib/api/client';
import { fetchOrError, fetchOrNull } from '$lib/api/load';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// 1件を取る API は無いので、一覧から探す (コンテンツの編集ページと同じ)。admin 限定であることは API と
// 上の `+layout.ts` が確かめる。
export const load: PageLoad = async ({ parent, params, url }) => {
	await parent();
	const id = Number(params.id);
	const [roots, users] = await Promise.all([
		fetchOrError(client.GET('/api/v1/admin/roots'), m.admin_roots_fetch_failed(), {
			loginRedirectFrom: url
		}),
		// 権限の区画で例外に足す候補。取れなければ、足せないことを区画に出す。
		fetchOrNull(client.GET('/api/v1/admin/users'))
	]);
	const root = roots.find((candidate) => candidate.id === id);
	if (!root) {
		error(404, m.error_not_found());
	}
	return { root, users };
};
