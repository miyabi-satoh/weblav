import { redirect } from '@sveltejs/kit';
import { resolve } from '$app/paths';
import { isAdmin } from '$lib/auth';
import type { LayoutLoad } from './$types';

// ユーザー管理は admin 限定 (→ docs/access.md「ロールと操作」)。`/admin/+layout.ts` が一律には
// 弾かない (コンテンツ管理は user にも開放されているため) のに対し、この画面自体が
// admin 専用なのでここでガードする。
export const load: LayoutLoad = async ({ parent }) => {
	const { user } = await parent();
	if (!isAdmin(user)) {
		redirect(303, resolve('/admin/contents'));
	}
	return {};
};
