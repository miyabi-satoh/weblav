import { client } from '#lib/api/client.ts';
import { fetchOrError, fetchOrNull } from '#lib/api/load.ts';
import { opFilters } from '#lib/operations.ts';
import * as m from '#lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// 新しいものから1回分を取り、続きは画面の「さらに読み込む」で足す。
// 絞り込みは URL クエリに持たせる。戻る操作と再読み込みで、選んだ絞り込みに戻れるため。
// ユーザーの一覧を先に取るのは、URL のユーザーが一覧にいるかで絞り込むかを決めるため。
export const load: PageLoad = async ({ parent, url }) => {
	await parent();
	// 絞り込みの欄に並べるユーザー。取れなければユーザーで絞り込めないだけにし、記録は見せる。
	const users = (await fetchOrNull(client.GET('/api/v1/admin/users'))) ?? [];
	const filters = opFilters(url.searchParams, users);
	const operations = await fetchOrError(
		client.GET('/api/v1/admin/operations', { params: { query: filters } }),
		m.admin_operations_fetch_failed(),
		{ loginRedirectFrom: url }
	);
	return { operations, users, filters };
};
