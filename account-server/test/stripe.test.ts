import { env } from 'cloudflare:workers';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	createCheckoutSession,
	endsAt,
	stripeConfig,
	usesManagedPayments,
	verifyWebhook
} from '../src/stripe';
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

describe('endsAt', () => {
	const periodEnd = 1_800_000_000;
	const sub = (over: { cancel_at?: number | null; cancel_at_period_end?: boolean }) => ({
		id: 'sub_1',
		status: 'active',
		customer: 'cus_1',
		metadata: null,
		items: { data: [{ id: 'si_1', current_period_end: periodEnd, price: { id: 'price_1' } }] },
		...over
	});

	it('is the cancel_at of a pending cancellation, or the period end with cancel_at_period_end', () => {
		expect(endsAt(sub({ cancel_at: periodEnd - 100 }))).toBe(periodEnd - 100);
		expect(endsAt(sub({ cancel_at_period_end: true }))).toBe(periodEnd);
	});

	it('is undefined while no cancellation is pending', () => {
		expect(endsAt(sub({}))).toBeUndefined();
		expect(endsAt(sub({ cancel_at: null, cancel_at_period_end: false }))).toBeUndefined();
	});
});

describe('usesManagedPayments', () => {
	it('sells directly only to Japan, and through Managed Payments when the country is unknown', () => {
		expect(usesManagedPayments('JP')).toBe(false);
		expect(usesManagedPayments('US')).toBe(true);
		expect(usesManagedPayments(undefined)).toBe(true);
	});
});

describe('createCheckoutSession', () => {
	afterEach(() => vi.restoreAllMocks());

	/** Stripe へ送った Checkout Session の項目。 */
	async function sentFor(email: string) {
		const stripe = vi
			.spyOn(globalThis, 'fetch')
			.mockImplementation(async () => Response.json({ id: 'cs_1', url: 'https://checkout.test' }));
		await createCheckoutSession(stripeConfig(env)!, {
			accountId: 'acct',
			email,
			interval: 'month',
			lang: 'ja',
			successUrl: 'https://account.test/done',
			cancelUrl: 'https://account.test/',
			expiresAt: 1_791_090_000,
			managedPayments: false,
			idempotencyKey: 'key'
		});
		return new URLSearchParams(String(stripe.mock.calls[0][1]!.body));
	}

	it('fills in the email of the account, except an Apple relay address', async () => {
		expect((await sentFor('buyer@example.com')).get('customer_email')).toBe('buyer@example.com');
		// 転送用アドレスの人は、支払いの画面でメールを入れる。
		for (const email of ['buyer123@privaterelay.appleid.com', 'buyer456@private.icloud.com']) {
			vi.restoreAllMocks();
			expect((await sentFor(email)).has('customer_email'), email).toBe(false);
		}
	});
});
