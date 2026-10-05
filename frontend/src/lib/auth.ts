import { withQuery } from '$lib/href';
import type { components } from '$lib/api/schema';

/**
 * ログインが必要なページの route id 判定。`+layout.ts` で使う。
 *
 * ログインを要求するのは `/admin` 配下だけ (→ docs/access.md「フロントエンドのガード反転」)。
 *
 * `url.pathname` ではなく `route.id` で判定する: base path を設定してもroute idは
 * 変わらないため。
 */
const PROTECTED_ROUTE_PREFIX = '/admin';

export function isProtectedRoute(routeId: string | null): boolean {
	return (
		routeId === PROTECTED_ROUTE_PREFIX ||
		(routeId?.startsWith(`${PROTECTED_ROUTE_PREFIX}/`) ?? false)
	);
}

/**
 * ログイン中のユーザーが admin か。未ログインは `false`。
 *
 * admin 限定の操作 (→ docs/access.md「ロールと操作」) を画面に出すかどうかの判定に使う。
 * 権限はサーバー側でも同じように検査するので、ここでの判定は操作できない項目を
 * 見せないためだけのもの。
 */
export function isAdmin(
	user: Pick<components['schemas']['UserResponse'], 'role'> | null | undefined
): boolean {
	return user?.role === 'admin';
}

/**
 * ログイン中のユーザーがコンテンツを書き換えられるか。`user` は自分が作ったものだけ
 * (→ docs/access.md「ロールと操作」)。`isAdmin` と同じく、見せる・見せないの判定にだけ使う。
 */
export function canEditContent(
	user: Pick<components['schemas']['UserResponse'], 'id' | 'role'> | null | undefined,
	content: Pick<components['schemas']['AdminContentResponse'], 'createdBy'>
): boolean {
	if (!user) return false;
	return isAdmin(user) || content.createdBy === user.id;
}

/**
 * ログイン後の戻り先を組み立てる。`/login?redirect=...` の形で渡す。
 */
export function loginPathWithRedirect(loginPath: string, target: string): string {
	return withQuery(loginPath, { redirect: target });
}

/**
 * 戻り先が API のパスか。
 *
 * ブラウザが直接開いたファイルの取得が 401 になると、サーバーは
 * `/login?redirect=/api/v1/...` へ送ってくる (→ `src/api/browser.rs`)。
 * API のパスは SvelteKit のルートではないので `goto` では解決できず、
 * ブラウザのナビゲーションとして開く必要がある。
 */
export function isApiTarget(target: string): boolean {
	return target.startsWith('/api/');
}

/**
 * `redirect` クエリの値を戻り先として使ってよいか検証する。
 *
 * 自サイト内の絶対パスだけを許可する。`//example.com` や `/\example.com` はブラウザから
 * 別オリジンへのスキーム相対URLとして解釈されるため、オープンリダイレクトになる。
 * 許可できない値は `null` を返し、呼び出し側で既定の遷移先へ落とす。
 */
export function safeRedirectTarget(target: string | null): string | null {
	if (target === null) return null;
	if (!target.startsWith('/')) return null;
	if (target.startsWith('//') || target.startsWith('/\\')) return null;
	return target;
}
