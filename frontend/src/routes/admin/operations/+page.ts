import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import { OP_CATEGORIES } from '$lib/operations';
import * as m from '$lib/paraglide/messages.js';
import type { components } from '$lib/api/schema';
import type { PageLoad } from './$types';

type OpCategory = components['schemas']['OpCategory'];

/** URL クエリの絞り込み。知らない値は絞り込まないものとして扱う。 */
function filtersOf(url: URL): { category?: OpCategory; userId?: number } {
	const category = OP_CATEGORIES.find((value) => value === url.searchParams.get('category'));
	const user = Number(url.searchParams.get('user'));
	return { category, userId: Number.isInteger(user) && user > 0 ? user : undefined };
}

// 新しいものから1回分を取り、続きは画面の「さらに読み込む」で足す。
// 絞り込みは URL クエリに持たせる。戻る操作と再読み込みで、選んだ絞り込みに戻れるため。
export const load: PageLoad = async ({ parent, url }) => {
	await parent();
	const filters = filtersOf(url);
	const [operations, users] = await Promise.all([
		fetchOrError(
			client.GET('/api/v1/admin/operations', { params: { query: filters } }),
			m.admin_operations_fetch_failed(),
			{ loginRedirectFrom: url }
		),
		fetchOrError(client.GET('/api/v1/admin/users'), m.admin_operations_fetch_failed(), {
			loginRedirectFrom: url
		})
	]);
	return { operations, users, filters };
};
