import { client } from '$lib/api/client';
import { fetchOrError, fetchOrNull } from '$lib/api/load';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// admin 限定であることは API が確かめる。追加だけは、サーバーの PC から開いたときに限る (+page.svelte)。
export const load: PageLoad = async ({ parent, url }) => {
	await parent();
	const [roots, users] = await Promise.all([
		fetchOrError(client.GET('/api/v1/admin/roots'), m.admin_roots_fetch_failed(), {
			loginRedirectFrom: url
		}),
		// 権限のダイアログの例外に並べるユーザー。取れなければ例外を選べないだけにする。
		fetchOrNull(client.GET('/api/v1/admin/users'))
	]);
	return { roots, users: users ?? [] };
};
