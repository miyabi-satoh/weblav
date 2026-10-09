import { redirect } from '@sveltejs/kit';
import { resolve } from '$app/paths';
import { client } from '$lib/api/client';
import { isApiTarget, safeRedirectTarget } from '$lib/auth';
import { isLoopbackHost } from '$lib/loopback';
import type { PageLoad } from './$types';

/**
 * 最初の管理者を作る手順への案内を出すか。サーバーの PC から開いていて、管理者がまだいないときだけ
 * (→ docs/help.md)。問い合わせ先はループバックからしか通らないので、ほかの端末では問い合わせない。
 * 問い合わせられないときは出さない (トレイの「セットアップ」と同じ)。
 */
async function setupRequired(url: URL): Promise<boolean> {
	if (!isLoopbackHost(url.hostname)) return false;
	try {
		const { response } = await client.GET('/api/v1/setup/status');
		return response.status === 200;
	} catch {
		return false;
	}
}

/**
 * ログイン済みでこのページを開いたときは、フォームを見せずに行き先へ送る。
 *
 * ヘッダーはログイン済みならログインリンクを出さないため、ここへ来るのは
 * ブックマークか直接入力に限られる。それでもフォームがそのまま出ると
 * 「ログアウトされたのか」と考えさせてしまう。ログイン済みを認証ページから
 * 追い出すのは一般的な作法でもある。
 *
 * 行き先は `?redirect=` があればそこ、無ければトップ。値の検証は
 * `safeRedirectTarget` に任せる (オープンリダイレクト対策)。
 * ログイン成功後の遷移 (`+page.svelte`) と同じ規則。
 */
export const load: PageLoad = async ({ parent, url }) => {
	const { user } = await parent();
	if (user === null) return { setupRequired: await setupRequired(url) };

	const target = safeRedirectTarget(url.searchParams.get('redirect'));
	// トップへ落とす場合:
	// - 戻り先が無い
	// - このページ自身。そこへ送ってもまたここへ戻り、`?redirect=` が落ちるまで
	//   余分に遷移するだけ (無限には続かない)
	// - API のパス。SvelteKit のルートではないので `redirect()` では送れない。
	//   ログイン済みならファイルは開き直せるので、ここで凝らない
	//   (ログインの成功後は `+page.svelte` がブラウザーに開かせる)
	if (
		target === null ||
		isApiTarget(target) ||
		new URL(target, url.origin).pathname === url.pathname
	) {
		redirect(307, resolve('/'));
	}
	// `target` はブラウザーのURLから取った実パスであり、route id ではないため
	// `resolve()` は通さない。
	redirect(307, target);
};
