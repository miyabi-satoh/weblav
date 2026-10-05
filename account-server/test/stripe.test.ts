import { env } from 'cloudflare:workers';
import { describe, expect, it } from 'vitest';
import { stripeConfig, usesManagedPayments, verifyWebhook } from '../src/stripe';
import { toHex, utf8 } from '../src/util';

describe('verifyWebhook', () => {
	const config = stripeConfig(env)!;
	const body = '{"id":"evt_1"}';
	const at = 1_791_090_000;

	/** Stripe と同じ形の `Stripe-Signature`。 */
	async function header(t: number, secret = config.webhookSecret) {
		const key = await crypto.subtle.importKey(
			'raw',
			utf8(secret),
			{ name: 'HMAC', hash: 'SHA-256' },
			false,
			['sign']
		);
		const mac = new Uint8Array(await crypto.subtle.sign('HMAC', key, utf8(`${t}.${body}`)));
		return `t=${t},v1=${toHex(mac)}`;
	}

	it('accepts a signature made with the secret within five minutes', async () => {
		expect(await verifyWebhook(config, body, await header(at), at)).toBe(true);
		expect(await verifyWebhook(config, body, await header(at - 300), at)).toBe(true);
		// 鍵を替える間は v1 が2つ付く。どちらかが合えば通す。
		const rolled = `${await header(at, 'whsec_old')},v1=${(await header(at)).split('v1=')[1]}`;
		expect(await verifyWebhook(config, body, rolled, at)).toBe(true);
	});

	it('rejects a wrong, stale, future or missing signature', async () => {
		const cases: [string, string | undefined][] = [
			['wrong secret', await header(at, 'whsec_wrong')],
			['stale', await header(at - 301)],
			['future', await header(at + 301)],
			['tampered signature', (await header(at)).replace(/v1=.{2}/, 'v1=00')],
			['empty signature', `t=${at},v1=`],
			['no timestamp', (await header(at)).replace(/^t=\d+,/, '')],
			['no header', undefined]
		];
		for (const [label, value] of cases) {
			expect(await verifyWebhook(config, body, value, at), label).toBe(false);
		}
	});
});

describe('usesManagedPayments', () => {
	it('sells directly only to Japan, and through Managed Payments when the country is unknown', () => {
		expect(usesManagedPayments('JP')).toBe(false);
		expect(usesManagedPayments('US')).toBe(true);
		expect(usesManagedPayments(undefined)).toBe(true);
	});
});
