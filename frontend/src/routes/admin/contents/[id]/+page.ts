import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import { isAdmin } from '$lib/auth';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// 作成者の選択肢 (admin だけに出す → docs/ui.md「UI 全般」)。
export const load: PageLoad = async ({ parent, url }) => {
	const { user } = await parent();
	if (!isAdmin(user)) return { users: [] };

	const users = await fetchOrError(
		client.GET('/api/v1/admin/users'),
		m.admin_users_fetch_failed(),
		{ loginRedirectFrom: url }
	);
	return { users };
};
