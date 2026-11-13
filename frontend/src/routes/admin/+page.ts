import { redirect } from '@sveltejs/kit';
import { resolve } from '$app/paths';
import type { PageLoad } from './$types';

// 管理画面の入口はコンテンツ管理 (→ docs/ui.md「UI 全般」)。`/admin` を直接開いても 404 にしない。
export const load: PageLoad = () => {
	redirect(307, resolve('admin/contents'));
};
