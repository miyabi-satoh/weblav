import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// admin 限定であることは +layout.ts が既に確認済み。ここでは一覧取得のみ行う。
export const load: PageLoad = async ({ parent, url }) => {
	await parent();

	const users = await fetchOrError(
		client.GET('/api/v1/admin/users'),
		m.admin_users_fetch_failed(),
		{ loginRedirectFrom: url }
	);
	return { users };
};
