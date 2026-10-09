import { error, redirect } from '@sveltejs/kit';
import { resolve } from '$app/paths';
import { client } from '$lib/api/client';
import { GENERIC_ERROR_MESSAGE } from '$lib/api/errors';
import { isProtectedRoute, loginPathWithRedirect } from '$lib/auth';
import * as m from '$lib/paraglide/messages.js';
import type { components } from '$lib/api/schema';
import type { LayoutLoad } from './$types';

// weblav.exe はビルド済みの静的ファイルを配信するだけなので、
// サーバーサイドレンダリングは行わず SPA として動かす。
export const ssr = false;

// ssr = false のため、これはブラウザーでのみ実行される。
// アプリ起動時・全ページ遷移時に `/auth/me` を呼び、ログイン状態を `user` として配る。
// 未ログイン(401)はエラーにせず `user: null` として扱い、`/admin` 配下だけログインへ
// 誘導する (→ docs/access.md「フロントエンドのガード反転」)。
export const load: LayoutLoad = async ({ route, url }) => {
	// **fetch より先に `route.id` と `url` を読む**。SvelteKit は load が実際に触れた
	// プロパティだけを依存として記録するため、401 のときだけ触る書き方だと、ログイン
	// できている間はどちらも依存に入らず、画面内の遷移でこの load が再実行されない。
	const routeId = route.id;
	const currentPath = url.pathname + url.search;

	let response: Response;
	let data: components['schemas']['UserResponse'] | undefined;
	try {
		({ data, response } = await client.GET('/api/v1/auth/me'));
	} catch {
		// DB ダウン等でセッションは有効なままかもしれないため、
		// 未ログイン扱いにして先へ進めるのは誤り。
		error(503, GENERIC_ERROR_MESSAGE());
	}

	// 401 のみ未ログインとみなす。5xx 等はセッションが有効な可能性があるため、
	// 匿名として扱わずエラーとして扱う。
	if (response.status === 401) {
		if (isProtectedRoute(routeId)) {
			redirect(307, loginPathWithRedirect(resolve('/login'), currentPath));
		}
		return { user: null };
	}
	if (!response.ok || !data) {
		// error() は 400〜599 の範囲しか受け付けないため、response.ok (200番台) なのに
		// data が無いという想定外のケースは 500 とする。
		error(response.ok ? 500 : response.status, m.layout_me_fetch_failed());
	}

	return { user: data };
};
