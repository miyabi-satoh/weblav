import { redirect } from '@sveltejs/kit';
import { resolve } from '$app/paths';
import { isAdmin } from '$lib/auth';
import type { LayoutLoad } from './$types';

// `/admin` の下は admin 専用 (→ docs/access.md「ロールと操作」)。未ログインは親(ルート `+layout.ts`)が
// `/login` へ送る。`user` が URL で開いたときは、API の 403 が「取得に失敗しました」と出て
// 不具合に見えないよう、ホームへ戻す。
// `user` も使うコンテンツの編集ページは `/contents/[id]` に置き、ここの下には置かない。
export const load: LayoutLoad = async ({ parent }) => {
	const { user } = await parent();
	if (!isAdmin(user)) {
		redirect(303, resolve('/'));
	}
	return {};
};
