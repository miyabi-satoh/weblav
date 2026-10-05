import { redirect } from '@sveltejs/kit';
import { resolve } from '$app/paths';
import { isAdmin } from '$lib/auth';
import type { LayoutLoad } from './$types';

// サイト設定は admin 限定 (→ docs/access.md「ロールと操作」)。ユーザー管理と同じく、この画面でガードする
// (`/admin/+layout.ts` は user にも開いているため一律には弾かない)。
export const load: LayoutLoad = async ({ parent }) => {
	const { user } = await parent();
	if (!isAdmin(user)) {
		redirect(303, resolve('/admin/contents'));
	}
	return {};
};
