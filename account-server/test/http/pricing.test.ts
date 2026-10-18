import { env } from 'cloudflare:workers';
import { describe, expect, it } from 'vitest';
import server from '../../src/index';

const PAGE =
	'<section><a class="buy" data-buy href="/account/buy?lang=ja">クレジットを購入</a>' +
	'<p data-sale-closed hidden>準備中</p></section><a href="/terms/">利用規約</a>';

/** テストには静的なページが無いので、料金のページだけを返す ASSETS を差し込む。 */
function withAssets(overrides: Partial<Env> = {}) {
	const requested: Request[] = [];
	const ASSETS = {
		fetch: async (input: RequestInfo | URL) => {
			requested.push(new Request(input));
			return new Response(PAGE, {
				headers: { 'content-type': 'text/html; charset=utf-8', etag: '"abc"' }
			});
		}
	} as unknown as Fetcher;
	return { env: { ...env, ASSETS, ...overrides } as Env, requested };
}

describe('GET /pricing/', () => {
	it('closes the sale while Stripe is not configured', async () => {
		const { env: closedEnv, requested } = withAssets({
			STRIPE_SECRET_KEY: undefined
		} as Partial<Env>);
		const res = await server.request(
			'/pricing/',
			{ headers: { 'if-none-match': '"abc"' } },
			closedEnv
		);
		expect(res.status).toBe(200);
		expect(await res.text()).not.toContain('/account/buy');
		// 条件付きで取ると 304 が返り、書き換える本文が無くなる。
		expect(requested[0].headers.get('if-none-match')).toBeNull();
	});

	it('serves the page as is once Stripe is configured', async () => {
		const { env: openEnv } = withAssets();
		const res = await server.request('/pricing/', {}, openEnv);
		expect(await res.text()).toContain('/account/buy');
		expect(res.headers.get('etag')).toBe('"abc"');
	});
});
