/**
 * Stripe での Pro の販売 (→ docs/pro.md「売り方」)。個人向けは Checkout のサブスク、組織向けは請求書を送るサブスクで売る。
 * SDK は使わず、要るもの (Checkout Session・サブスク・請求書・カスタマーポータル・webhook の署名) だけを fetch と Web Crypto で書く。
 */

import { fromHex, isEmail, normalizeEmail, utf8 } from './util';
import type { Plan } from './link';

/** 送られてから受け付けるまでの猶予。古い webhook を送り直されても通さないように (Stripe の SDK の既定と同じ)。 */
const WEBHOOK_TOLERANCE = 300;

/**
 * 同じ Stripe のアカウントで売るほかの製品の webhook と見分ける印。サブスクの metadata と購入の台帳に付ける。
 * 組織向けの印は、運営者がダッシュボードで作るサブスクの metadata に付ける。
 */
const PRODUCTS: Record<Plan, string> = { personal: 'weblav-pro', organization: 'weblav-org' };

/**
 * Sign in with Apple の転送用アドレスのドメイン。新しく出るアドレスの private.icloud.com と、
 * 前から出ていて今も使われる privaterelay.appleid.com の両方を見る。
 */
const APPLE_RELAY_DOMAINS = ['privaterelay.appleid.com', 'private.icloud.com'];

export function productOf(plan: Plan): string {
	return PRODUCTS[plan];
}

/** Stripe が断った頼み。`status` が 409 なら、同じ Idempotency-Key の頼みを Stripe が処理している最中。 */
export class StripeError extends Error {
	constructor(
		readonly status: number,
		body: string
	) {
		super(`Stripe responded ${status}: ${body}`);
	}
}

/** 個人向けの払い方。 */
export type Interval = 'month' | 'year';

export type StripeConfig = {
	secretKey: string;
	webhookSecret: string;
	personalPrices: Record<Interval, string>;
	taxRateId: string;
	/** 組織向けの Price。無いのに組織向けの請求書が届いたら、500 で送り直してもらう。 */
	orgPriceId?: string;
};

/** 秘密の値と Price・税率がそろったときだけ売る。手元で動かすときは無くてよい (Pro は `just dev-account-grant` で付ける)。 */
export function stripeConfig(env: Env): StripeConfig | undefined {
	const {
		STRIPE_SECRET_KEY,
		STRIPE_WEBHOOK_SECRET,
		STRIPE_PERSONAL_MONTHLY_PRICE_ID,
		STRIPE_PERSONAL_YEARLY_PRICE_ID,
		STRIPE_TAX_RATE_ID
	} = env;
	if (
		!STRIPE_SECRET_KEY ||
		!STRIPE_WEBHOOK_SECRET ||
		!STRIPE_PERSONAL_MONTHLY_PRICE_ID ||
		!STRIPE_PERSONAL_YEARLY_PRICE_ID ||
		!STRIPE_TAX_RATE_ID
	) {
		return undefined;
	}
	return {
		secretKey: STRIPE_SECRET_KEY,
		webhookSecret: STRIPE_WEBHOOK_SECRET,
		personalPrices: {
			month: STRIPE_PERSONAL_MONTHLY_PRICE_ID,
			year: STRIPE_PERSONAL_YEARLY_PRICE_ID
		},
		taxRateId: STRIPE_TAX_RATE_ID,
		orgPriceId: env.STRIPE_ORG_PRICE_ID || undefined
	};
}

/** Price からプランを引く。この製品の Price でなければ `undefined`。 */
function planOfPrice(config: StripeConfig, priceId: string | undefined): Plan | undefined {
	if (!priceId) return undefined;
	if (priceId === config.personalPrices.month || priceId === config.personalPrices.year) {
		return 'personal';
	}
	if (priceId === config.orgPriceId) return 'organization';
	return undefined;
}

/**
 * 日本からの買い手には直接、ほかの国からの買い手には Managed Payments で売る (→ docs/pro.md「売り方」)。
 * 国はアクセス元の IP で決める。分からなければ MP にする (MP は日本の買い手にも売れる)。
 */
export function usesManagedPayments(country: string | undefined): boolean {
	return country !== 'JP';
}

async function stripeFetch<T>(
	config: StripeConfig,
	method: 'GET' | 'POST' | 'DELETE',
	path: string,
	params?: URLSearchParams,
	idempotencyKey?: string
): Promise<T> {
	const headers: Record<string, string> = { authorization: `Bearer ${config.secretKey}` };
	if (idempotencyKey) headers['idempotency-key'] = idempotencyKey;
	const query = method === 'GET' && params ? `?${params}` : '';
	const res = await fetch(`https://api.stripe.com/v1/${path}${query}`, {
		method,
		headers,
		body: method === 'GET' ? undefined : params
	});
	if (!res.ok) throw new StripeError(res.status, await res.text());
	return res.json<T>();
}

/**
 * 個人向けの Checkout Session (サブスク) を作る。`expiresAt` (UNIX 秒) は Stripe の決まりで30分以上先。
 * 同じ `idempotencyKey` で頼み直すと、Stripe は最初に作った Session を返す (24時間まで)。
 * 持ち主は、サブスクの metadata のアカウントの id で引く (→ index.ts の invoice.paid)。
 */
export async function createCheckoutSession(
	config: StripeConfig,
	{
		accountId,
		email,
		interval,
		lang,
		successUrl,
		cancelUrl,
		expiresAt,
		submitMessage,
		managedPayments,
		idempotencyKey
	}: {
		accountId: string;
		email: string;
		interval: Interval;
		lang: string;
		successUrl: string;
		cancelUrl: string;
		expiresAt: number;
		/** 支払いのボタンの下に出す文言。MP の Session では送れないので、国内の分だけ渡す。 */
		submitMessage?: string;
		managedPayments: boolean;
		idempotencyKey: string;
	}
): Promise<{ id: string; url: string }> {
	const params = new URLSearchParams({
		mode: 'subscription',
		'line_items[0][price]': config.personalPrices[interval],
		'line_items[0][quantity]': '1',
		client_reference_id: accountId,
		'metadata[product]': productOf('personal'),
		'subscription_data[metadata][product]': productOf('personal'),
		'subscription_data[metadata][account_id]': accountId,
		// 台帳に MP の取引かを残すため (請求書からは引けない)。
		'subscription_data[metadata][managed_payments]': managedPayments ? '1' : '0',
		locale: lang,
		success_url: successUrl,
		cancel_url: cancelUrl,
		expires_at: String(expiresAt),
		// 指定しないとアカウントの既定 (MP が有効) になるので、国内の分も明示する。
		'managed_payments[enabled]': String(managedPayments)
	});
	// MP の Session では、払い方・税・明細の表記・支払いの画面の文言を Stripe が決め、指定すると断られる。
	if (!managedPayments) {
		for (const [key, value] of Object.entries({
			// カードだけにする。後から払う方法 (コンビニ払いなど) は入金まで日がかかり、戻り先の画面で待ち切れない。
			'payment_method_types[0]': 'card',
			// 領収書を適格簡易請求書にするための、税率と税額 (税込み 10%)。
			'line_items[0][tax_rates][0]': config.taxRateId
		})) {
			params.set(key, value);
		}
	}
	// Checkout は渡したメールを直させない。Apple の転送用アドレスには、登録していない Stripe からの領収書が届かないので、
	// 渡さずに支払いの画面で入れてもらう。
	if (!APPLE_RELAY_DOMAINS.some((domain) => email.endsWith(`@${domain}`))) {
		params.set('customer_email', email);
	}
	// 特定商取引法 12条の6 の最終確認画面に要る、契約の期間・解約・引き渡し・返金の扱い。Markdown のリンクを書ける。
	// MP の分は、同じ事項を買う画面 (→ src/pages.ts の buyForm) にだけ出す。
	if (submitMessage) params.set('custom_text[submit][message]', submitMessage);
	return stripeFetch(config, 'POST', 'checkout/sessions', params, idempotencyKey);
}

/** 開いている支払いの画面を閉じる。別の払い方で押し直したときに、2つの画面で二重に払わせないように。 */
export async function expireCheckoutSession(config: StripeConfig, sessionId: string) {
	try {
		await stripeFetch(config, 'POST', `checkout/sessions/${encodeURIComponent(sessionId)}/expire`);
	} catch (e) {
		// もう払い終えた・閉じた画面は閉じられない。払い終えていれば、知らせで Pro が付く。
		if (!(e instanceof StripeError && e.status === 400)) throw e;
	}
}

export type Subscription = {
	id: string;
	status: string;
	customer: string;
	metadata: Record<string, string> | null;
	items: { data: { price: { id: string } }[] };
};

export function getSubscription(config: StripeConfig, id: string): Promise<Subscription> {
	return stripeFetch(config, 'GET', `subscriptions/${encodeURIComponent(id)}`);
}

/** その場で解約する (返金・不審請求・アカウントの削除)。もう終わっていれば何もしない。 */
export async function cancelSubscription(config: StripeConfig, id: string) {
	try {
		await stripeFetch(config, 'DELETE', `subscriptions/${encodeURIComponent(id)}`);
	} catch (e) {
		if (!(e instanceof StripeError && (e.status === 404 || e.status === 400))) throw e;
	}
}

/** カスタマーポータル (解約・支払い方法の変更・領収書)。戻り先はアカウントのページ。 */
export async function billingPortalUrl(
	config: StripeConfig,
	customer: string,
	returnUrl: string,
	lang: string
): Promise<string> {
	const session = await stripeFetch<{ url: string }>(
		config,
		'POST',
		'billing_portal/sessions',
		new URLSearchParams({ customer, return_url: returnUrl, locale: lang })
	);
	return session.url;
}

/** 付けてよいと確かめた、払われた請求書。台帳に残し、サブスクの払い終えた期間を延ばす。 */
export type PaidInvoice = {
	invoiceId: string;
	subscription: Subscription;
	plan: Plan;
	paymentIntentId: string;
	/** 払われた額と通貨 (最小の単位で、円ならそのまま)。 */
	amount: number;
	currency: string;
	/** この請求書の期間の終わり (UNIX 秒)。 */
	periodEnd: number;
	managedPayments: boolean;
	/** カードで払ったときの発行国。振り分けや返金には使わない。 */
	cardCountry: string | null;
};

/**
 * この製品の請求書なのに付けられないもの。請求書の作り方の誤りなので、運営者に知らせて手で直してもらう。
 * `message` は運営者が読むわけ。
 */
export class InvoiceError extends Error {}

type Charge = {
	payment_method_details: { card?: { country: string | null } } | null;
};

type Invoice = {
	id: string;
	status: string;
	currency: string;
	total: number;
	amount_paid: number;
	parent: { subscription_details?: { subscription: string } | null } | null;
	lines: {
		has_more: boolean;
		data: {
			amount: number;
			quantity: number | null;
			period: { start: number; end: number };
			pricing: { price_details?: { price: string } } | null;
			discount_amounts: { amount: number }[];
		}[];
	};
	payments: {
		has_more: boolean;
		data: {
			status: string;
			amount_paid: number | null;
			payment: {
				type: string;
				payment_intent?: {
					id: string;
					latest_charge: string | null;
				};
			};
		}[];
	};
};

/**
 * 知らせの本文は信じず、請求書とサブスクを Stripe から取り直して、付けてよい支払いかを確かめる。
 * この製品のものでない・払われていないときは `undefined`。
 * この製品の Price なのに、項目が1つ (数量 1・値引き無し) でない・Stripe で全額を1回で払っていないなら `InvoiceError`。
 */
export async function confirmInvoice(
	config: StripeConfig,
	invoiceId: string
): Promise<PaidInvoice | undefined> {
	const invoice = await stripeFetch<Invoice>(
		config,
		'GET',
		`invoices/${encodeURIComponent(invoiceId)}`,
		new URLSearchParams([
			// Stripe の expand は4段まで。カードの発行国は支払いを別に取る。
			['expand[]', 'payments.data.payment.payment_intent'],
			['expand[]', 'parent.subscription_details']
		])
	);
	const subscriptionId = invoice.parent?.subscription_details?.subscription;
	if (invoice.status !== 'paid' || !subscriptionId) return undefined;
	const lines = invoice.lines.data;
	const plan = planOfPrice(config, lines[0]?.pricing?.price_details?.price);
	if (!plan) return undefined;
	const subscription = await getSubscription(config, subscriptionId);
	const product = productOf(plan);
	if (subscription.metadata?.product !== product) {
		throw new InvoiceError(`サブスクの metadata の product が ${product} でない。`);
	}
	const paid = invoice.payments.data.filter((p) => p.status === 'paid');
	const intent = paid[0]?.payment.payment_intent;
	if (
		invoice.lines.has_more ||
		lines.length !== 1 ||
		lines[0].quantity !== 1 ||
		lines[0].discount_amounts.some((d) => d.amount !== 0) ||
		lines[0].amount !== invoice.total ||
		invoice.amount_paid !== invoice.total ||
		invoice.payments.has_more ||
		paid.length !== 1 ||
		paid[0].payment.type !== 'payment_intent' ||
		!intent ||
		paid[0].amount_paid !== invoice.total
	) {
		throw new InvoiceError(
			'項目が WebLAV Pro の Price の1つ (数量 1・値引き無し) でないか、Stripe で全額を1回で払ったものでない。'
		);
	}
	const charge = intent.latest_charge
		? await stripeFetch<Charge>(
				config,
				'GET',
				`charges/${encodeURIComponent(intent.latest_charge)}`
			)
		: null;
	return {
		invoiceId: invoice.id,
		subscription,
		plan,
		paymentIntentId: intent.id,
		amount: invoice.total,
		currency: invoice.currency,
		periodEnd: lines[0].period.end,
		managedPayments: subscription.metadata?.managed_payments === '1',
		cardCountry: charge?.payment_method_details?.card?.country ?? null
	};
}

/** 組織向けのサブスクの metadata の `account_email`。無い・形が違えば `InvoiceError`。 */
export function orgAccountEmail(subscription: Subscription): string {
	const email = normalizeEmail(subscription.metadata?.account_email ?? '');
	if (!isEmail(email)) {
		throw new InvoiceError(
			'組織向けのサブスクの metadata の account_email に、WebLAV を使う人のメールアドレスが無い。'
		);
	}
	return email;
}

/**
 * `Stripe-Signature: t=<秒>,v1=<署名>[,v1=...]` を確かめる。署名は `<秒>.<本文>` の HMAC-SHA256。
 * 本文は受け取ったままのバイト列で確かめる (JSON を読み直すと並びが変わって通らない)。
 */
export async function verifyWebhook(
	config: StripeConfig,
	body: string,
	header: string | undefined,
	nowSeconds: number
): Promise<boolean> {
	const parts = (header ?? '').split(',').map((part) => part.trim().split('='));
	const timestamp = parts.find(([k]) => k === 't')?.[1];
	const signatures = parts.filter(([k]) => k === 'v1').map(([, v]) => v);
	if (!timestamp || signatures.length === 0) return false;
	if (Math.abs(nowSeconds - Number(timestamp)) > WEBHOOK_TOLERANCE) return false;
	const key = await crypto.subtle.importKey(
		'raw',
		utf8(config.webhookSecret),
		{ name: 'HMAC', hash: 'SHA-256' },
		false,
		['verify']
	);
	const signed = utf8(`${timestamp}.${body}`);
	for (const signature of signatures) {
		const bytes = fromHex(signature);
		// 比べるのは verify に任せる (中で時間の揃った比較をする)。空の署名は通さない。
		if (bytes?.length && (await crypto.subtle.verify('HMAC', key, bytes, signed))) return true;
	}
	return false;
}
