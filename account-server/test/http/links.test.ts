import { env } from 'cloudflare:workers';
import { describe, expect, it } from 'vitest';
import {
	app,
	approve,
	exchange,
	linkApp,
	linkPath,
	newLink,
	postForm,
	request,
	signIn
} from '../helpers';

describe('linking a PC', () => {
	it('signs in first, approves on the account page, and hands the token to the polling WebLAV', async () => {
		const email = 'owner@example.com';
		const { verifier, fields } = await newLink('教室の WebLAV', 'Taro の MacBook');

		// 認める前の問い合わせは、待つように返す。
		expect((await exchange(verifier)).status).toBe(202);

		// サインインしていなければ、サインインの画面を出し、終わったら登録の画面へ戻す。
		const before = await request(linkPath(fields));
		expect(await before.text()).toContain('WebLAV にサインイン');
		const { cookie, location } = await signIn(email, linkPath(fields));
		expect(location).toBe(linkPath(fields));

		const page = await (await request(location!, { cookie })).text();
		expect(page).toContain(
			'PC「Taro の MacBook」の WebLAV「教室の WebLAV」を、このアカウントに登録します'
		);
		expect(page).toContain('この PC を登録');

		expect(await (await approve(fields, cookie)).text()).toContain(
			'「Taro の MacBook」を登録しました'
		);
		const answer = await exchange(verifier);
		expect(answer.status).toBe(200);
		const { token } = await answer.json<{ token: string }>();
		expect(token).toMatch(/^[0-9a-f]{64}$/);
		// トークンそのものは持たず、ハッシュだけを持つ。
		expect(
			await env.DB.prepare('SELECT 1 FROM app_tokens WHERE token_hash = ?').bind(token).first()
		).toBeNull();
		expect(await (await app('/v1/balance', token)).json()).toEqual({
			email,
			remaining_percent: 100
		});
		// 窓口の画面で、どの PC の WebLAV かが分かる。
		expect(await (await request('/account/', { cookie })).text()).toContain(
			'Taro の MacBook の「教室の WebLAV」（'
		);
	});

	it('hands out a token once, and only to the verifier', async () => {
		const { cookie } = await signIn('verifier@example.com');
		const { verifier, fields } = await newLink();
		await approve(fields, cookie);
		// 検証用の値が違えば、認めた印は見つからない。
		expect((await exchange('f'.repeat(64))).status).toBe(202);
		expect((await exchange(verifier)).status).toBe(200);
		// 一度替えたら、同じ検証用の値ではもう替えられない。
		expect((await exchange(verifier)).status).toBe(202);
		expect((await exchange('not-hex')).status).toBe(400);
	});

	it('keeps the first approval when the same request is approved from another account', async () => {
		const first = await signIn('first-approver@example.com');
		const second = await signIn('second-approver@example.com');
		const { verifier, fields } = await newLink();
		await approve(fields, first.cookie);
		await approve(fields, second.cookie);
		const { token } = await (await exchange(verifier)).json<{ token: string }>();
		expect(await (await app('/v1/balance', token)).json()).toMatchObject({
			email: 'first-approver@example.com'
		});
	});

	it('does not hand out a token for an expired approval', async () => {
		const { cookie } = await signIn('late-code@example.com');
		const { verifier, fields } = await newLink();
		await approve(fields, cookie);
		await env.DB.prepare(
			"UPDATE link_approvals SET expires_at = 1 WHERE account_id = (SELECT id FROM accounts WHERE email = 'late-code@example.com')"
		).run();
		expect((await exchange(verifier)).status).toBe(202);
	});

	// 申し込みの値の確かめ (src/index.ts の linkRequest) は export されていないので、ここで場合ごとに確かめる。
	it('accepts only a well-formed challenge, and falls back to default names', async () => {
		const { cookie } = await signIn('shape@example.com');
		const { fields } = await newLink();
		for (const bad of [{ challenge: 'short' }, { challenge: 'G'.repeat(64) }]) {
			const query = { ...fields, ...bad };
			expect((await request(linkPath(query), { cookie })).status).toBe(400);
			expect((await postForm('/account/link', query, cookie)).status).toBe(400);
		}
		// 名前が無くても登録できる。窓口の画面には「WebLAV」「PC」と出す。
		const { verifier, fields: unnamed } = await newLink('  ', '');
		await approve(unnamed, cookie);
		expect((await exchange(verifier)).status).toBe(200);
		expect(await (await request('/account/', { cookie })).text()).toContain('PC の「WebLAV」（');
	});
});

describe('app tokens', () => {
	it('signs WebLAV out, and the token stops working', async () => {
		const { token } = await linkApp('signout@example.com');
		expect((await app('/v1/token', token, { method: 'DELETE' })).status).toBe(204);
		expect((await app('/v1/balance', token)).status).toBe(401);
	});

	it('lists linked WebLAV on the account page and removes one from there', async () => {
		const { cookie, token } = await linkApp('apps@example.com');
		const home = await (await request('/account/', { cookie })).text();
		expect(home).toContain('登録している PC');
		const id = home.match(/name="id" value="([0-9a-f]+)"/)![1];
		// ほかのアカウントからは外せない。
		const other = await signIn('not-mine@example.com');
		await postForm('/account/apps/unlink', { id }, other.cookie);
		expect((await app('/v1/balance', token)).status).toBe(200);
		expect((await postForm('/account/apps/unlink', { id }, cookie)).status).toBe(303);
		expect((await app('/v1/balance', token)).status).toBe(401);
		expect(await (await request('/account/', { cookie })).text()).toContain(
			'登録している PC はありません'
		);
	});
});
