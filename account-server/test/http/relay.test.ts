import { env } from 'cloudflare:workers';
import { describe, expect, it, vi } from 'vitest';
import { costOf } from '../../src/ai';
import { pricing } from '../../src/pricing';
import { searchCost } from '../../src/search';
import { accountId, app, geminiAnswers, grantsOf, linkApp, sendAi, sendSearch } from '../helpers';

const { freeGrant: FREE_GRANT, rates } = pricing(env);

describe('relaying to Gemini', () => {
	it('returns what Gemini answers, and charges what it reports', async () => {
		const email = 'relay@example.com';
		const { token } = await linkApp(email);
		const gemini = geminiAnswers();
		const res = await sendAi(token, { system: '丁寧に', user: 'こんにちは' });
		expect(res.status).toBe(200);
		// Gemini の返事をそのまま返す (WebLAV が利用者のキーで送ったときと同じく読む)。
		expect(await res.json()).toMatchObject({ status: 'completed' });
		expect(JSON.parse(String(gemini.mock.calls[0][1]!.body))).toMatchObject({
			input: 'こんにちは',
			system_instruction: '丁寧に'
		});
		expect((await grantsOf(await accountId(email)))[0].remaining).toBe(
			FREE_GRANT - costOf({ total_input_tokens: 1000, total_output_tokens: 800 }, rates)!
		);
		// 使った量を、1回の購入で付く量を 100万とした数で知らせる。
		expect(res.headers.get('x-weblav-used')).toBe(
			String(
				Math.ceil(
					(costOf({ total_input_tokens: 1000, total_output_tokens: 800 }, rates)! * 1_000_000) /
						pricing(env).purchaseGrant
				)
			)
		);
	});

	it('passes an attached file to Gemini with the text', async () => {
		const { token } = await linkApp('attach@example.com');
		const gemini = geminiAnswers();
		const pdf = { mime_type: 'application/pdf', data: 'JVBERi0=' };
		expect((await sendAi(token, { user: '要約して', attachments: [pdf] })).status).toBe(200);
		expect(JSON.parse(String(gemini.mock.calls[0][1]!.body)).input).toEqual([
			{
				type: 'user_input',
				content: [
					{ type: 'document', data: 'JVBERi0=', mime_type: 'application/pdf' },
					{ type: 'text', text: '要約して' }
				]
			}
		]);
	});

	it('refuses without credit, without sending', async () => {
		const email = 'empty@example.com';
		const { token } = await linkApp(email);
		const account = await accountId(email);
		await env.DB.prepare('UPDATE grants SET remaining = 0 WHERE account_id = ?')
			.bind(account)
			.run();
		const gemini = geminiAnswers();
		const refused = await sendAi(token);
		expect(refused.status).toBe(402);
		expect(await refused.json()).toEqual({ error: 'no_credit' });
		expect(gemini).not.toHaveBeenCalled();
		expect(await (await app('/v1/balance', token)).json()).toMatchObject({ remaining_percent: 0 });
	});

	it('does not charge when Gemini cannot be reached, and charges failures Gemini reports', async () => {
		const email = 'unreachable@example.com';
		const { token } = await linkApp(email);
		vi.spyOn(globalThis, 'fetch').mockRejectedValue(new TypeError('network down'));
		expect((await sendAi(token)).status).toBe(500);
		expect((await grantsOf(await accountId(email)))[0].remaining).toBe(FREE_GRANT);
		vi.restoreAllMocks();
		vi.spyOn(console, 'error').mockImplementation(() => {});
		geminiAnswers({ total_input_tokens: 1000, total_output_tokens: 0 }, 429);
		const failed = await sendAi(token);
		expect(failed.status).toBe(502);
		expect(await failed.json()).toEqual({ error: 'upstream', upstream_status: 429 });
		expect((await grantsOf(await accountId(email)))[0].remaining).toBe(
			FREE_GRANT - costOf({ total_input_tokens: 1000, total_output_tokens: 0 }, rates)!
		);
		// 失敗の後も、次の中継を受ける。
		vi.restoreAllMocks();
		geminiAnswers();
		expect((await sendAi(token)).status).toBe(200);
	});

	it('takes overlapping requests from the same account, and charges each', async () => {
		const email = 'overlap@example.com';
		const { token } = await linkApp(email);
		geminiAnswers();
		const answers = await Promise.all([sendAi(token), sendAi(token), sendAi(token)]);
		expect(answers.map((res) => res.status)).toEqual([200, 200, 200]);
		expect((await grantsOf(await accountId(email)))[0].remaining).toBe(
			FREE_GRANT - 3 * costOf({ total_input_tokens: 1000, total_output_tokens: 800 }, rates)!
		);
	});

	it('limits how fast one account can call', async () => {
		const { token } = await linkApp('ai-limit@example.com');
		geminiAnswers();
		// 上限 (10秒に20回) は時計の区切りごとに数えるので、またいでも片側で超える数を送る。
		const statuses: number[] = [];
		for (let n = 0; n < 41 && !statuses.includes(429); n++) {
			statuses.push((await sendAi(token)).status);
		}
		expect(statuses[0]).toBe(200);
		expect(statuses).toContain(429);
	});

	it('refuses malformed and too long requests without sending them', async () => {
		// どの形を断るかは test/ai.test.ts の parsePrompt で確かめる。
		const { token } = await linkApp('malformed@example.com');
		const gemini = geminiAnswers();
		const malformed = await sendAi(token, { text: 'x' });
		expect(malformed.status).toBe(400);
		expect(await malformed.json()).toEqual({ error: 'bad_request' });
		const tooLong = await sendAi(token, { user: 'x'.repeat(200_001) });
		expect(tooLong.status).toBe(413);
		expect(await tooLong.json()).toEqual({ error: 'too_long' });
		expect(gemini).not.toHaveBeenCalled();
	});
});

describe('relaying a web search', () => {
	const found = {
		results: [
			{
				title: 'トマトの育て方',
				url: 'https://example.com/tomato',
				date: '2026-04-16',
				snippet: '抜粋'
			},
			{ title: '日付なし', url: 'https://example.org/a', last_updated: '2026-08-05', snippet: '' },
			{ title: '形の違う URL', url: 'javascript:alert(1)', snippet: '' }
		]
	};

	it('returns the candidates, and charges one request', async () => {
		const email = 'search@example.com';
		const { token } = await linkApp(email);
		const provider = vi
			.spyOn(globalThis, 'fetch')
			.mockImplementation(async () => Response.json(found));
		const res = await sendSearch(token, {
			queries: ['家庭菜園 トマト'],
			country: 'JP',
			language: 'ja',
			recency: 'year'
		});
		expect(res.status).toBe(200);
		expect(await res.json()).toEqual({
			results: [
				{
					title: 'トマトの育て方',
					url: 'https://example.com/tomato',
					date: '2026-04-16',
					snippet: '抜粋'
				},
				{ title: '日付なし', url: 'https://example.org/a', date: '2026-08-05', snippet: '' }
			]
		});
		expect(JSON.parse(String(provider.mock.calls[0][1]!.body))).toMatchObject({
			query: ['家庭菜園 トマト'],
			search_type: 'fast',
			country: 'JP',
			search_language_filter: ['ja'],
			search_recency_filter: 'year'
		});
		expect((await grantsOf(await accountId(email)))[0].remaining).toBe(
			FREE_GRANT - searchCost(rates)
		);
		expect(Number(res.headers.get('x-weblav-used'))).toBeGreaterThan(0);
	});

	it('does not charge when the provider fails', async () => {
		const email = 'search-fail@example.com';
		const { token } = await linkApp(email);
		vi.spyOn(console, 'error').mockImplementation(() => {});
		vi.spyOn(globalThis, 'fetch').mockImplementation(
			async () => new Response('nope', { status: 500 })
		);
		const failed = await sendSearch(token, { queries: ['x'] });
		expect(failed.status).toBe(502);
		expect(await failed.json()).toEqual({ error: 'upstream', upstream_status: 500 });
		expect((await grantsOf(await accountId(email)))[0].remaining).toBe(FREE_GRANT);
	});

	it('refuses malformed requests without sending them', async () => {
		// どの形を断るかは test/search.test.ts の parseSearch で確かめる。
		const { token } = await linkApp('search-malformed@example.com');
		const provider = vi.spyOn(globalThis, 'fetch');
		const malformed = await sendSearch(token, { queries: [] });
		expect(malformed.status).toBe(400);
		expect(await malformed.json()).toEqual({ error: 'bad_request' });
		expect(provider).not.toHaveBeenCalled();
	});
});
