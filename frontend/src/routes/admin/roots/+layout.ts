import { redirect } from '@sveltejs/kit';
import { resolve } from '$app/paths';
import { isAdmin } from '$lib/auth';
import type { LayoutLoad } from './$types';

// 公開できるフォルダーは admin 限定 (→ docs/folders.md「公開できるフォルダー」)。ユーザー管理と同じく、この画面でガードする
// (`/admin/+layout.ts` は user にも開いているため一律には弾かない)。
// 戻さないと、user が URL で開いたときに API の 403 が「取得に失敗しました」と出て、不具合に見える。
export const load: LayoutLoad = async ({ parent }) => {
	const { user } = await parent();
	if (!isAdmin(user)) {
		redirect(303, resolve('/admin/contents'));
	}
	return {};
};
