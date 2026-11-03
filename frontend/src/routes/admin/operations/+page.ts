import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// 新しいものから1回分を取り、続きは画面の「さらに読み込む」で足す。
export const load: PageLoad = async ({ parent, url }) => {
	await parent();
	const operations = await fetchOrError(
		client.GET('/api/v1/admin/share-operations'),
		m.admin_operations_fetch_failed(),
		{ loginRedirectFrom: url }
	);
	return { operations };
};
