/**
 * WebLAV の窓口 (→ docs/pro.md「アカウントと販売の窓口」)。
 *
 * - `/v1/installations/`: 結んだ WebLAV のサーバーからの確かめと外す (→ docs/pro.md「窓口との形」)。
 * - `/v1/stripe/webhook`: Stripe からの支払いの知らせ。
 * - `/account/`: 人が開く画面 (サインイン・結ぶ・申し込む・移す)。
 * - それ以外: 紹介と規約類の静的なページ (site/ をビルドしたもの。wrangler.jsonc の assets)。
 */
import { Hono, type Context } from 'hono';
import { csrf } from 'hono/csrf';
import { formatDate, formatMoney, messages, resolveLang, type Lang } from './i18n';
import { sendMail } from './mail';
import {
	alreadyProPage,
	checkingPurchasePage,
	confirmPage,
	confirmSignInPage,
	homePage,
	installationName,
	LEGAL_PAGES,
	linkAtLimitPage,
	linkedPage,
	linkPage,
	mailSentPage,
	messagePage,
	noProPage,
	planSwitchPage,
	releasePage,
	transferConfirmPage,
	transferPage,
	transferredPage,
	signInPage,
	type InstallationRow,
	type PlanSwitch,
	type SaleRegion
} from './pages';
import { finishGoogleSignIn, googleConfig, startGoogleSignIn } from './google';
import { appleConfig, finishAppleSignIn, startAppleSignIn } from './apple';
import {
	checkAuth,
	deriveSecret,
	grantCode,
	grantExpiresAt,
	installationId,
	linkKey,
	parseRelease,
	parseRequest,
	relinkTag,
	releaseTag,
	REQUEST_TTL,
	sameBytes,
	signProof,
	type Plan
} from './link';
import {
	billingPortalUrl,
	cancelSubscription,
	confirmInvoice,
	createCheckoutSession,
	expireCheckoutSession,
	getSubscription,
	intervalOf,
	InvoiceError,
	managedPaymentsOf,
	orgAccountEmail,
	previewYearlySwitch,
	productOf,
	releaseSchedule,
	scheduleMonthly,
	StripeError,
	stripeConfig,
	switchToYearly,
	type Interval,
	type PaidInvoice,
	type StripeConfig,
	type Subscription,
	usesManagedPayments,
	verifyWebhook
} from './stripe';
import { currentAccount, endSession, startSession, type Account } from './session';
import {
	ACCOUNT,
	ACCOUNT_HOME,
	formString,
	fromHex,
	isEmail,
	normalizeEmail,
	now,
	originOf,
	PLAN_PATH,
	PRICING_PATH,
	randomHex,
	safeNext,
	sha256Hex,
	toHex,
	TRANSFER_PATH
} from './util';

/** 結べる台数の上限 (→ docs/pro.md「結ぶ」)。 */
const PLAN_LIMITS: Record<Plan, number> = { personal: 3, organization: 10 };
/**
 * 払い終えた期間の終わりから、許可を出し続ける猶予 (→ docs/pro.md「結び付きと許可」)。
 * カードは支払いの失敗を Stripe が試し直す間、組織向けは請求書の支払い期日 (30日) まで。
 */
const GRACE: Record<Plan, number> = { personal: 7 * 86_400, organization: 30 * 86_400 };
/** サイト名を付けていない WebLAV の名前。 */
const DEFAULT_NAME = 'WebLAV';
/** 窓口に出す名前の上限。WebLAV のサイト名の上限より長く取る。 */
const NAME_MAX = 100;
/** メールのリンクの期限。メールが少し遅れて届いても間に合い、漏れたリンクを使える時間は短く。 */
const EMAIL_LOGIN_TTL = 15 * 60;
/** 同じアドレスへ1時間に送るリンクの上限。他人のアドレスへ送りつけるのに使われないように。 */
const EMAIL_LOGINS_PER_HOUR = 5;
/** 上の上限を数える幅。上限を「1時間に何通」で決めているので、その1時間。 */
const EMAIL_LOGIN_WINDOW = 3600;
/** 支払いから戻った先で、Pro が付くのを自動で確かめ直す回数 (3秒おき)。webhook は数秒で届くことが多い。 */
const PURCHASE_CHECKS = 10;
/**
 * 支払いの画面の期限。Stripe が受け付ける最も短い30分に、同じ予約で頼み直せる5分を足す
 * (Stripe は頼むたびに期限が30分以上先かを確かめる)。
 */
const CHECKOUT_TTL = 35 * 60;
/**
 * アカウントを消したあと、購入の記録を残す期間。適格請求書の写しと帳簿の保存期間 (7年) に、うるう年の分を足す。
 */
const PURCHASE_RETENTION = 7 * 366 * 24 * 60 * 60;

type App = { Bindings: Env };
// `/account` と `/account/` を同じ画面にする (紹介のページは末尾に `/` を付けてリンクする)。
const app = new Hono<App>({ strict: false });

/**
 * 送り主 (IP) ごとの上限に当たったら true。認証の要らない書き込みで、D1 の書き込みの枠やメールを使い切られないように。
 * 数え方はおおよそ (Workers の Rate Limiting)。正確さより、止めることを取る。
 */
async function limited(c: Context<App>, limiter: RateLimit, key?: string): Promise<boolean> {
	const { success } = await limiter.limit({
		key: key ?? c.req.header('cf-connecting-ip') ?? 'unknown'
	});
	return !success;
}

// ---- Pro と結び付き ----

type PlanRow = {
	id: string;
	plan: Plan;
	paid_through: number;
	stripe_customer_id: string | null;
	status: string;
};

/** アカウントのサブスクの行のうち、打ち切っていないもの。猶予を過ぎた行も含む (有効かは `isLive` で見る)。新しく払った順。 */
async function subscriptionsOf(env: Env, accountId: string): Promise<PlanRow[]> {
	const { results } = await env.DB.prepare(
		`SELECT id, plan, paid_through, stripe_customer_id, status FROM subscriptions
		 WHERE account_id = ? AND revoked_at IS NULL ORDER BY paid_through DESC`
	)
		.bind(accountId)
		.all<PlanRow>();
	return results;
}

/**
 * 有効な期間の終わり。払い終えた期間の終わりに猶予を足す。
 * 解約した行は払われることがもう無いので、猶予を足さない (足すと、解約して期間が終わった人が猶予の間は申し込み直せない)。
 */
function liveUntil(row: PlanRow): number {
	return row.status === 'canceled' ? row.paid_through : row.paid_through + GRACE[row.plan];
}

function isLive(row: PlanRow, at: number): boolean {
	return liveUntil(row) > at;
}

/** 月額と年額を切り替えられる行。窓口の Checkout で買った、個人向けの Stripe のサブスクで、解約していないもの。 */
function switchable(row: PlanRow, at: number): boolean {
	return (
		row.plan === 'personal' &&
		row.stripe_customer_id !== null &&
		row.status !== 'canceled' &&
		isLive(row, at)
	);
}

/**
 * アカウントの今の Pro (→ docs/pro.md「結び付きと許可」の「期限」)。いつも全部のサブスクの行から求める。
 * 組織向けが有効ならそれ、無ければ個人向け。期限は、そのプランの有効な行のうち最も遅いもの。
 */
async function activePlan(
	env: Env,
	accountId: string,
	at: number
): Promise<{ plan: Plan; expiresAt: number } | undefined> {
	return bestPlan(await subscriptionsOf(env, accountId), at);
}

function bestPlan(rows: PlanRow[], at: number): { plan: Plan; expiresAt: number } | undefined {
	let best: { plan: Plan; expiresAt: number } | undefined;
	for (const row of rows) {
		if (!isLive(row, at)) continue;
		const expiresAt = liveUntil(row);
		if (
			!best ||
			(row.plan === 'organization' && best.plan === 'personal') ||
			(row.plan === best.plan && expiresAt > best.expiresAt)
		) {
			best = { plan: row.plan, expiresAt };
		}
	}
	return best;
}

type Installation = {
	id: string;
	account_id: string;
	link_kid: number;
	public_key: string;
	name: string;
	pc_name: string;
	created_at: number;
	checked_at: number | null;
	removed_at: number | null;
	permission_expires_at: number | null;
};

/**
 * ADR: ネットで受け取る証明の期限の上限。WebLAV は30日おきに確かめるので、つながる PC は切れる前に延びる。
 * 年額の期限まで延ばすと、壊れて確かめに来ない PC を外しても、その枠が最長1年空かない。
 */
const ONLINE_PROOF_LIFETIME = 45 * 86_400;

/**
 * アカウントの結び付きを、古く結んだ順に。外して許可の期限も過ぎた行は先に消す (もう台数に数えない)。
 * `overLimit` は、外していない行のうち上限の台数より後ろのもの (→ docs/pro.md「結ぶ」)。
 */
async function installationsOf(env: Env, accountId: string, limit: number, at: number) {
	await purgeRemoved(env, accountId, at).run();
	const { results } = await env.DB.prepare(
		'SELECT * FROM installations WHERE account_id = ? ORDER BY created_at, rowid'
	)
		.bind(accountId)
		.all<Installation>();
	// 外したが WebLAV が受け取っていない行も枠を使うので、外していない行に回せる数はその分だけ減る。
	let allowed = limit - results.filter((r) => r.removed_at !== null).length;
	return results.map((row) => {
		const overLimit = row.removed_at === null && allowed-- <= 0;
		return { row, overLimit };
	});
}

/** 外して許可の期限も過ぎた行を消す文。 */
function purgeRemoved(env: Env, accountId: string, at: number) {
	return env.DB.prepare(
		`DELETE FROM installations
		 WHERE account_id = ? AND removed_at IS NOT NULL AND coalesce(permission_expires_at, 0) <= ?`
	).bind(accountId, at);
}

function installationRows(list: { row: Installation; overLimit: boolean }[]): InstallationRow[] {
	return list.map(({ row, overLimit }) => ({
		id: row.id,
		name: installationName(row.name, row.pc_name),
		checkedAt: row.checked_at,
		removedUntil: row.removed_at === null ? null : row.permission_expires_at,
		overLimit
	}));
}

function cleanName(name: string | undefined): string {
	return (name ?? '').trim().slice(0, NAME_MAX);
}

// ---- WebLAV のサーバーからの確かめと外す (→ docs/pro.md「窓口との形」) ----

app.post('/v1/installations/check', async (c) => {
	if (await limited(c, c.env.INSTALLATION_LIMITER)) return c.json({ error: 'rate_limited' }, 429);
	const body = await c.req
		.json<{ installation?: unknown; auth?: unknown; name?: unknown; pc?: unknown }>()
		.catch(() => ({}) as never);
	if (typeof body.installation !== 'string' || typeof body.auth !== 'string') {
		return c.json({ error: 'invalid_request' }, 400);
	}
	const at = now();
	const found = await installationWithSecret(c.env, body.installation);
	const auth = fromHex(body.auth);
	if (!found || !auth || !sameBytes(auth, await checkAuth(found.secret))) {
		return c.json({ status: 'unbound' });
	}
	const { row } = found;
	// 窓口で外した行は、WebLAV がこの答えを受け取って外れるので、ここで消して枠を空ける。
	if (row.removed_at !== null) {
		await releaseInstallation(c.env, row, at);
		return c.json({ status: 'unbound' });
	}
	const name = cleanName(typeof body.name === 'string' ? body.name : undefined);
	const pcName = cleanName(typeof body.pc === 'string' ? body.pc : undefined);
	await c.env.DB.prepare(
		`UPDATE installations SET checked_at = ?1,
		   name = CASE WHEN ?2 = '' THEN name ELSE ?2 END,
		   pc_name = CASE WHEN ?3 = '' THEN pc_name ELSE ?3 END
		 WHERE id = ?4`
	)
		.bind(at, name, pcName, row.id)
		.run();
	const account = (await c.env.DB.prepare('SELECT email FROM accounts WHERE id = ?')
		.bind(row.account_id)
		.first<{ email: string }>())!;
	const active = await activePlan(c.env, row.account_id, at);
	if (!active) return c.json({ status: 'no_plan', email: account.email });
	const list = await installationsOf(c.env, row.account_id, PLAN_LIMITS[active.plan], at);
	if (list.find((i) => i.row.id === row.id)?.overLimit) {
		return c.json({ status: 'over_limit', email: account.email });
	}
	const expiresAt = Math.min(active.expiresAt, at + ONLINE_PROOF_LIFETIME);
	const proof = await signProof(c.env, {
		account: row.account_id,
		installation: row.id,
		plan: active.plan,
		issuedAt: at,
		expiresAt
	});
	await recordPermission(c.env, row.id, expiresAt);
	return c.json({ status: 'ok', proof, email: account.email });
});

app.post('/v1/installations/release', async (c) => {
	if (await limited(c, c.env.INSTALLATION_LIMITER)) return c.json({ error: 'rate_limited' }, 429);
	const body = await c.req.json<{ code?: unknown }>().catch(() => ({}) as never);
	if (typeof body.code === 'string') await release(c.env, body.code);
	return c.body(null, 204);
});

/** 外した証しを確かめて、結び付きを消す。確かめられなければ何もしない。 */
async function release(env: Env, code: string): Promise<boolean> {
	const parsed = parseRelease(code);
	if (!parsed) return false;
	const found = await installationWithSecret(env, parsed.installation);
	if (!found || !sameBytes(parsed.tag, await releaseTag(found.secret))) return false;
	await releaseInstallation(env, found.row, now());
	return true;
}

/** 結び付きを消し、その公開鍵を覚える (申し込みの期限が過ぎるまで、同じ申し込みでは結ばない)。 */
function releaseInstallation(env: Env, row: Installation, at: number) {
	return env.DB.batch([
		env.DB.prepare('DELETE FROM installations WHERE id = ?').bind(row.id),
		env.DB.prepare(
			'INSERT INTO released_keys (public_key, released_at) VALUES (?, ?) ON CONFLICT DO UPDATE SET released_at = excluded.released_at'
		).bind(row.public_key, at)
	]);
}

/** 出した許可の期限を覚える。外した後も、この期限までは台数に数える。 */
function recordPermission(env: Env, id: string, expiresAt: number) {
	return env.DB.prepare(
		'UPDATE installations SET permission_expires_at = max(coalesce(permission_expires_at, 0), ?) WHERE id = ?'
	)
		.bind(expiresAt, id)
		.run();
}

/** id の結び付きと、その秘密。行が無いか、今の鍵で結んだものでなければ `undefined`。 */
async function installationWithSecret(
	env: Env,
	id: string
): Promise<{ row: Installation; secret: Uint8Array } | undefined> {
	const row = await env.DB.prepare('SELECT * FROM installations WHERE id = ?')
		.bind(id)
		.first<Installation>();
	const secret = row ? await secretOf(env, row) : undefined;
	return row && secret ? { row, secret } : undefined;
}

/** 行の公開鍵から秘密を導き直す。今の鍵で結んだものでなければ `undefined`。 */
async function secretOf(env: Env, row: Installation): Promise<Uint8Array | undefined> {
	const key = await linkKey(env);
	const pub = fromHex(row.public_key);
	if (row.link_kid !== key.kid || !pub) return undefined;
	return deriveSecret(key, pub);
}

// ---- Stripe からの知らせ (→ docs/pro.md「売り方」) ----

/**
 * Pro を付ける・延ばすのはここだけ。支払いの画面から戻った先では付けない (戻らずに閉じられることもあるため)。
 * 知らせは id で記録し、処理し終えたものは送り直されても処理し直さない。失敗したら 500 を返し、Stripe の送り直しで処理し直す。
 * どの処理も、何度行っても結果が同じになるように書く (記録する前に落ちても送り直されるため)。
 */
app.post('/v1/stripe/webhook', async (c) => {
	const config = stripeConfig(c.env);
	if (!config) return c.json({ error: 'not_found' }, 404);
	const body = await c.req.text();
	if (!(await verifyWebhook(config, body, c.req.header('stripe-signature'), now()))) {
		return c.json({ error: 'invalid_signature' }, 400);
	}
	const event = JSON.parse(body) as {
		id: string;
		type: string;
		data: { object: Record<string, unknown> };
	};
	const seen = await c.env.DB.prepare('SELECT status FROM stripe_events WHERE id = ?')
		.bind(event.id)
		.first<{ status: string }>();
	if (seen?.status === 'done') return c.json({ received: true });
	await c.env.DB.prepare(
		`INSERT INTO stripe_events (id, type, status, received_at, updated_at) VALUES (?1, ?2, 'received', ?3, ?3)
		 ON CONFLICT (id) DO UPDATE SET status = 'received', updated_at = ?3`
	)
		.bind(event.id, event.type, now())
		.run();
	let outcome: 'done' | 'failed';
	try {
		outcome = (await handleStripeEvent(c.env, config, event.type, event.data.object)) ?? 'done';
	} catch (e) {
		await markStripeEvent(c.env, event.id, 'failed');
		throw e;
	}
	await markStripeEvent(c.env, event.id, outcome);
	// 片付けは次の知らせのときにもやり直せるので、失敗しても知らせは受け取ったことにする。
	await forgetOldPurchases(c.env).catch((e) => console.error('failed to forget old purchases', e));
	return c.json({ received: true });
});

/**
 * アカウントから外れて保存の期間を過ぎた購入の記録を消す。期間は外した日時から数える (アカウントが無いまま
 * 届いた購入は、購入の日時から)。知らせは少ないので、受けるたびに片付ける。
 */
function forgetOldPurchases(env: Env) {
	const before = now() - PURCHASE_RETENTION;
	return env.DB.batch([
		env.DB.prepare(
			`DELETE FROM purchases WHERE account_id IS NULL AND coalesce(detached_at, created_at) <= ?`
		).bind(before),
		env.DB.prepare('DELETE FROM stripe_revoked_payments WHERE created_at <= ?').bind(before)
	]);
}

function markStripeEvent(env: Env, id: string, status: 'done' | 'failed') {
	return env.DB.prepare('UPDATE stripe_events SET status = ?, updated_at = ? WHERE id = ?')
		.bind(status, now(), id)
		.run();
}

type SubscriptionRow = {
	id: string;
	account_id: string | null;
	plan: Plan;
	stripe_customer_id: string | null;
	paid_through: number;
	status: string;
	revoked_at: number | null;
};

/**
 * 同じ Stripe のアカウントのほかの製品の知らせも届くので、Price とサブスクの行で見分け、ほかは何もしない。
 * 運営者が手で直すまで処理できない知らせは `'failed'` を返す。Stripe には受け取ったと返し、直した後に送り直してもらう。
 */
async function handleStripeEvent(
	env: Env,
	config: StripeConfig,
	type: string,
	object: Record<string, unknown>
): Promise<'failed' | void> {
	switch (type) {
		case 'invoice.paid': {
			let paid: PaidInvoice | undefined;
			let owner: string | undefined;
			try {
				paid = await confirmInvoice(config, String(object.id));
				if (!paid) return;
				const row = await subscriptionRow(env, paid.subscription.id);
				if (!row) owner = await newSubscriptionOwner(env, paid);
			} catch (e) {
				if (!(e instanceof InvoiceError)) throw e;
				// 払われたのに付けられない。運営者が気づけるよう知らせる (送れなければ 500 で送り直してもらう)。
				await sendMail(
					env,
					env.OPERATOR_EMAIL,
					'[WebLAV] 請求書で Pro を付けられませんでした',
					[
						`請求書 ${String(object.id)} は払われましたが、Pro を付けていません。`,
						`わけ: ${e.message}`,
						'',
						'直し方は運営の手順の「組織向けの請求書を出すとき」。'
					].join('\n')
				);
				return 'failed';
			}
			await recordPaidInvoice(env, paid, owner);
			// 返金・不審請求の知らせが先に届いていたら、そのときはサブスクを引けず解約できていない。
			const revoked = await env.DB.prepare(
				'SELECT 1 FROM stripe_revoked_payments WHERE payment_intent_id = ?'
			)
				.bind(paid.paymentIntentId)
				.first();
			if (revoked) await cancelSubscription(config, paid.subscription.id);
			return;
		}
		// 状態だけを合わせる。知らせの順は決まっていないので、本文でなく取り直したサブスクで合わせ、終わった行は戻さない。
		case 'customer.subscription.updated':
		case 'customer.subscription.deleted': {
			const row = await subscriptionRow(env, String(object.id));
			if (!row || row.status === 'canceled') return;
			const subscription = await getSubscription(config, row.id);
			await env.DB.prepare(
				"UPDATE subscriptions SET status = ? WHERE id = ? AND status != 'canceled'"
			)
				.bind(subscription.status, row.id)
				.run();
			return;
		}
		// 全額を返金したとき・不審請求を申し立てられたときは、サブスクを打ち切る (→ docs/pro.md「売り方」)。
		// 出した許可は取り消せないので、結んだ WebLAV は次の確かめか許可の期限で Free に戻る。
		case 'charge.refunded':
		case 'charge.dispute.created': {
			if (type === 'charge.refunded' && object.refunded !== true) return;
			if (typeof object.payment_intent !== 'string') return;
			// 付ける知らせが後から届いても延ばさないよう、取り消した支払いを覚えておく。台帳の行は消さない。
			const at = now();
			const purchase = await env.DB.prepare(
				'SELECT stripe_subscription_id FROM purchases WHERE stripe_payment_intent_id = ?'
			)
				.bind(object.payment_intent)
				.first<{ stripe_subscription_id: string | null }>();
			await env.DB.batch([
				env.DB.prepare(
					`INSERT INTO stripe_revoked_payments (payment_intent_id, created_at) VALUES (?, ?)
					 ON CONFLICT DO NOTHING`
				).bind(object.payment_intent, at),
				env.DB.prepare(
					`UPDATE purchases SET revoked_at = ?
					 WHERE stripe_payment_intent_id = ? AND revoked_at IS NULL`
				).bind(at, object.payment_intent),
				env.DB.prepare(
					'UPDATE subscriptions SET revoked_at = coalesce(revoked_at, ?) WHERE id = ?'
				).bind(at, purchase?.stripe_subscription_id ?? '')
			]);
			if (purchase?.stripe_subscription_id) {
				await cancelSubscription(config, purchase.stripe_subscription_id);
			}
			return;
		}
	}
}

function subscriptionRow(env: Env, id: string) {
	return env.DB.prepare('SELECT * FROM subscriptions WHERE id = ?')
		.bind(id)
		.first<SubscriptionRow>();
}

/**
 * まだ行の無いサブスクの持ち主 (→ docs/pro.md「売り方」)。個人向けは申し込んだアカウントの id、
 * 組織向けは metadata の `account_email` のアカウント (無ければ作る)。
 * サブスクがもう終わっている (アカウントを消したときに解約した) なら `undefined` で、台帳にだけ残す。
 */
async function newSubscriptionOwner(env: Env, paid: PaidInvoice): Promise<string | undefined> {
	if (paid.subscription.status === 'canceled') return undefined;
	if (paid.plan === 'personal') {
		const accountId = paid.subscription.metadata?.account_id ?? '';
		const account = await env.DB.prepare('SELECT id FROM accounts WHERE id = ?')
			.bind(accountId)
			.first<{ id: string }>();
		return account?.id;
	}
	const email = orgAccountEmail(paid.subscription);
	const account = await upsertAccount(env, email, now()).first<{ id: string }>();
	return account!.id;
}

/**
 * 払われた請求書を台帳に残し、サブスクの払い終えた期間を延ばす。行が無ければ (最初の請求書) 作る。
 * 延ばすのは、打ち切っていない行で、この請求書の支払いを取り消していないときだけ。古い請求書では縮めない。
 * 1つのトランザクションにし、確かめる処理と書く処理を分けない。
 */
async function recordPaidInvoice(env: Env, paid: PaidInvoice, newOwner: string | undefined) {
	const at = now();
	const revokedPayment =
		'(SELECT created_at FROM stripe_revoked_payments WHERE payment_intent_id = ?)';
	await env.DB.batch([
		env.DB.prepare(
			`INSERT INTO subscriptions (id, account_id, plan, stripe_customer_id, paid_through, status, created_at)
			 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7 WHERE ?2 IS NOT NULL AND ${revokedPayment.replace('?', '?8')} IS NULL
			 ON CONFLICT DO NOTHING`
		).bind(
			paid.subscription.id,
			newOwner ?? null,
			paid.plan,
			paid.subscription.customer,
			paid.periodEnd,
			paid.subscription.status,
			at,
			paid.paymentIntentId
		),
		env.DB.prepare(
			`UPDATE subscriptions SET paid_through = max(paid_through, ?1), stripe_customer_id = ?2
			 WHERE id = ?3 AND revoked_at IS NULL AND ${revokedPayment.replace('?', '?4')} IS NULL`
		).bind(paid.periodEnd, paid.subscription.customer, paid.subscription.id, paid.paymentIntentId),
		env.DB.prepare(
			`INSERT INTO purchases
			   (id, account_id, product, stripe_invoice_id, stripe_subscription_id, stripe_payment_intent_id,
			    amount, currency, managed_payments, card_country, created_at, revoked_at)
			 VALUES (?1, (SELECT account_id FROM subscriptions WHERE id = ?2), ?3, ?4, ?2, ?5, ?6, ?7, ?8, ?9, ?10,
			   ${revokedPayment.replace('?', '?5')})
			 ON CONFLICT DO NOTHING`
		).bind(
			randomHex(16),
			paid.subscription.id,
			productOf(paid.plan),
			paid.invoiceId,
			paid.paymentIntentId,
			paid.amount,
			paid.currency,
			paid.managedPayments ? 1 : 0,
			paid.cardCountry,
			at
		)
	]);
}

// ---- 人が開く画面 ----

/** Apple から戻る先。 */
const APPLE_CALLBACK = `${ACCOUNT}/login/apple/callback`;

const accountApp = new Hono<App>();
accountApp.use(
	csrf({
		// Apple はサインインの結果を、Apple のサイトから戻り先へ POST で送る。送り元が `null` で届くこともあるので、
		// 戻り先だけは送り元を見ず、state の照らし合わせで守る (→ src/apple.ts)。
		origin: (origin, c) => origin === originOf(c) || c.req.path === APPLE_CALLBACK
	})
);

accountApp.get('/', async (c) => {
	const lang = resolveLang(c);
	const account = await currentAccount(c);
	if (!account) return c.html(signIn(c, lang, ACCOUNT_HOME));
	const at = now();
	const rows = await subscriptionsOf(c.env, account.id);
	const plans = rows
		.filter((r) => isLive(r, at))
		.map((r) => ({ plan: r.plan, paidThrough: r.paid_through }));
	const active = bestPlan(rows, at);
	const limit = PLAN_LIMITS[active?.plan ?? 'personal'];
	const list = await installationsOf(c.env, account.id, limit, at);
	const billing = stripeConfig(c.env) !== undefined;
	return c.html(
		homePage(lang, account.email, {
			plans,
			billing: billing && rows.some((r) => r.stripe_customer_id),
			switchable: billing && rows.some((r) => switchable(r, at)),
			limit,
			installations: installationRows(list),
			forSale: billing
		})
	);
});

accountApp.post('/login/email', async (c) => {
	const lang = resolveLang(c);
	const t = messages[lang];
	const form = await c.req.parseBody();
	const next = safeNext(formString(form, 'next'));
	const email = normalizeEmail(formString(form, 'email') ?? '');
	if (!isEmail(email)) {
		return c.html(signIn(c, lang, next, t.invalidEmail), 400);
	}
	if (await limited(c, c.env.EMAIL_LIMITER)) {
		return c.html(signIn(c, lang, next, t.tooManyLinks), 429);
	}
	const at = now();
	const token = randomHex(32);
	// 数えるのと足すのを1つの文にする。別々だと、同時に送られたときに上限を超える。
	const [, inserted] = await c.env.DB.batch([
		c.env.DB.prepare('DELETE FROM email_logins WHERE created_at <= ?').bind(
			at - EMAIL_LOGIN_WINDOW
		),
		c.env.DB.prepare(
			`INSERT INTO email_logins (token_hash, email, next, expires_at, created_at)
			 SELECT ?1, ?2, ?3, ?4, ?5
			 WHERE (SELECT count(*) FROM email_logins WHERE email = ?2 AND created_at > ?6) < ?7`
		).bind(
			await sha256Hex(token),
			email,
			next,
			at + EMAIL_LOGIN_TTL,
			at,
			at - EMAIL_LOGIN_WINDOW,
			EMAIL_LOGINS_PER_HOUR
		)
	]);
	if (inserted.meta.changes === 0) {
		return c.html(signIn(c, lang, next, t.tooManyLinks), 429);
	}
	// 別のブラウザで開いても同じ言語になるよう、リンクに言語を付ける。
	await sendMail(
		c.env,
		email,
		t.mailSubject,
		t.mailBody(
			`${originOf(c)}${ACCOUNT}/login/email?token=${token}&lang=${lang}`,
			EMAIL_LOGIN_TTL / 60
		)
	);
	return c.html(mailSentPage(lang, email, EMAIL_LOGIN_TTL / 60));
});

// メールのリンクを開いただけではサインインしない。メールのサービスがリンクを先に開いて確かめることがあり、
// そこで1回きりのトークンを使い切らないように。
accountApp.get('/login/email', (c) =>
	c.html(confirmSignInPage(resolveLang(c), c.req.query('token') ?? ''))
);

accountApp.post('/login/email/verify', async (c) => {
	const lang = resolveLang(c);
	const t = messages[lang];
	const form = await c.req.parseBody();
	const token = formString(form, 'token') ?? '';
	const at = now();
	const login = await c.env.DB.prepare(
		`UPDATE email_logins SET used_at = ?
		 WHERE token_hash = ? AND used_at IS NULL AND expires_at > ?
		 RETURNING email, next`
	)
		.bind(at, await sha256Hex(token), at)
		.first<{ email: string; next: string }>();
	if (!login) {
		return c.html(messagePage(lang, t.linkUnusableTitle, t.linkUnusable), 400);
	}
	const account = await upsertAccount(c.env, login.email, at).first<{ id: string }>();
	await startSession(c, account!.id);
	return c.redirect(safeNext(login.next), 303);
});

// ---- Google でサインイン (→ docs/pro.md「アカウントと販売の窓口」) ----

accountApp.get('/login/google', async (c) => {
	const config = googleConfig(c.env);
	if (!config) return c.notFound();
	const next = safeNext(c.req.query('next'));
	return c.redirect(await startGoogleSignIn(c, config, googleRedirectUri(c), next), 303);
});

accountApp.get('/login/google/callback', async (c) => {
	const lang = resolveLang(c);
	const t = messages[lang];
	const config = googleConfig(c.env);
	if (!config) return c.notFound();
	const user = await finishGoogleSignIn(c, config, googleRedirectUri(c), now());
	if ('failure' in user) {
		const error = user.failure === 'unconfirmed_email' ? t.googleUnconfirmed : t.googleFailed;
		return c.html(signIn(c, lang, user.next, error), 400);
	}
	return externalSignedIn(c, lang, 'google', user, t.googleConflict);
});

function googleRedirectUri(c: Context<App>): string {
	return `${originOf(c)}${ACCOUNT}/login/google/callback`;
}

// ---- Apple でサインイン (→ docs/pro.md「アカウントと販売の窓口」) ----

accountApp.get('/login/apple', (c) => {
	const config = appleConfig(c.env);
	if (!config) return c.notFound();
	const next = safeNext(c.req.query('next'));
	return c.redirect(startAppleSignIn(c, config, appleRedirectUri(c), next, resolveLang(c)), 303);
});

accountApp.post('/login/apple/callback', async (c) => {
	const config = appleConfig(c.env);
	if (!config) return c.notFound();
	const user = await finishAppleSignIn(c, config, appleRedirectUri(c), now());
	const lang = user.lang ?? resolveLang(c);
	const t = messages[lang];
	if ('failure' in user) {
		// 取り消したときは、何も言わずにサインインの画面へ戻す。
		return user.failure === 'cancelled'
			? c.html(signIn(c, lang, user.next))
			: c.html(signIn(c, lang, user.next, t.appleFailed), 400);
	}
	return externalSignedIn(c, lang, 'apple', user, t.appleConflict);
});

function appleRedirectUri(c: Context<App>): string {
	return `${originOf(c)}${APPLE_CALLBACK}`;
}

type Provider = 'google' | 'apple';

/** 外部のサインインで確かめたアカウントでサインインし、元の画面へ戻す。結べなければ `conflict` を出す。 */
async function externalSignedIn(
	c: Context<App>,
	lang: Lang,
	provider: Provider,
	user: { subject: string; email: string; next: string },
	conflict: string
) {
	const accountId = await externalAccount(c.env, provider, user.subject, user.email);
	if (!accountId) return c.html(signIn(c, lang, user.next, conflict), 409);
	await startSession(c, accountId);
	return c.redirect(safeNext(user.next), 303);
}

/**
 * 外部のサインインのアカウントに結ぶ窓口のアカウント (→ docs/pro.md「アカウントと販売の窓口」)。確かめ済みのメールだけを渡す。
 * 識別子 (sub) → 同じメールアドレスのアカウント → 新しいアカウント、の順で探す。
 * 同じメールのアカウントに、同じ方法の別のアカウントがもう結ばれていれば結ばず、`undefined` を返す。
 */
async function externalAccount(
	env: Env,
	provider: Provider,
	subject: string,
	email: string
): Promise<string | undefined> {
	const linked = await identity(env, provider, subject);
	if (linked) return linked.account_id;
	const at = now();
	const [account] = await env.DB.batch<{ id: string }>([
		// メールのリンクと同じく、同じアドレスなら同じアカウント。
		upsertAccount(env, email, at),
		env.DB.prepare(
			`INSERT INTO identities (provider, subject, account_id, created_at)
			 SELECT ?1, ?2, id, ?3 FROM accounts
			 WHERE email = ?4
			   AND NOT EXISTS (SELECT 1 FROM identities
			                   WHERE provider = ?1 AND account_id = accounts.id)
			 ON CONFLICT DO NOTHING`
		).bind(provider, subject, at, email)
	]);
	const id = account.results[0].id;
	return (await identity(env, provider, subject))?.account_id === id ? id : undefined;
}

function identity(env: Env, provider: Provider, subject: string) {
	return env.DB.prepare('SELECT account_id FROM identities WHERE provider = ? AND subject = ?')
		.bind(provider, subject)
		.first<{ account_id: string }>();
}

/** メールアドレスのアカウントを作るか、あればそれを返す文。どの方法で入っても、同じアドレスなら同じアカウント。 */
function upsertAccount(env: Env, email: string, at: number) {
	return env.DB.prepare(
		`INSERT INTO accounts (id, email, created_at) VALUES (?, ?, ?)
		 ON CONFLICT (email) DO UPDATE SET email = excluded.email
		 RETURNING id`
	).bind(randomHex(16), email, at);
}

/** メールアドレスのアカウントが無ければ作る文。batch の中で、続く文がメールアドレスから引けるように。 */
function ensureAccount(env: Env, email: string, at: number) {
	return env.DB.prepare(
		'INSERT INTO accounts (id, email, created_at) VALUES (?, ?, ?) ON CONFLICT (email) DO NOTHING'
	).bind(randomHex(16), email, at);
}

/** サインインの画面。Google・Apple でサインインできるときは、そのボタンも出す。 */
function signIn(c: Context<App>, lang: Lang, next: string, error?: string) {
	return signInPage(lang, next, {
		error,
		google: googleConfig(c.env) !== undefined,
		apple: appleConfig(c.env) !== undefined
	});
}

accountApp.post('/logout', async (c) => {
	const form = await c.req.parseBody();
	await endSession(c);
	return c.redirect(safeNext(formString(form, 'next')), 303);
});

// ---- WebLAV を結ぶ (→ docs/pro.md「結ぶ」) ----

type LinkTarget =
	| { kind: 'invalid' }
	| { kind: 'other_account' }
	/** 同じ申し込みをもう通した。返しのコードを出し直すだけ。 */
	| { kind: 'existing'; row: Installation; secret: Uint8Array }
	| {
			kind: 'new';
			secret: Uint8Array;
			id: string;
			kid: number;
			publicKey: string;
			/** 同じ WebLAV の結び直しなら、前の行。 */
			previous?: Installation;
	  };

/** 申し込みを読み、どの結び付きになるかを決める。期限切れ・外した公開鍵・形の違いは `invalid`。 */
async function linkTarget(
	env: Env,
	account: Account,
	raw: string,
	at: number
): Promise<LinkTarget> {
	const request = parseRequest(raw);
	const key = await linkKey(env);
	// 作った日時は、PC の時計が少し進んでいても受ける。
	if (
		!request ||
		request.kid !== key.kid ||
		request.createdAt < at - REQUEST_TTL ||
		request.createdAt > at + 3600
	) {
		return { kind: 'invalid' };
	}
	const secret = await deriveSecret(key, request.publicKey);
	if (!secret) return { kind: 'invalid' };
	const publicKey = toHex(request.publicKey);
	const existing = await env.DB.prepare('SELECT * FROM installations WHERE public_key = ?')
		.bind(publicKey)
		.first<Installation>();
	if (existing) {
		return existing.account_id === account.id
			? { kind: 'existing', row: existing, secret }
			: { kind: 'other_account' };
	}
	const released = await env.DB.prepare(
		'SELECT 1 FROM released_keys WHERE public_key = ? AND released_at > ?'
	)
		.bind(publicKey, at - REQUEST_TTL)
		.first();
	if (released) return { kind: 'invalid' };
	let previous: Installation | undefined;
	if (request.relink) {
		const row = await env.DB.prepare('SELECT * FROM installations WHERE id = ? AND account_id = ?')
			.bind(request.relink.installation, account.id)
			.first<Installation>();
		const previousSecret = row ? await secretOf(env, row) : undefined;
		if (
			row &&
			previousSecret &&
			sameBytes(request.relink.tag, await relinkTag(previousSecret, request.publicKey))
		) {
			previous = row;
		}
	}
	return {
		kind: 'new',
		secret,
		id: await installationId(secret),
		kid: key.kid,
		publicKey,
		previous
	};
}

function linkUrl(raw: string, name: string, pcName: string) {
	return `${ACCOUNT}/link?${new URLSearchParams({ r: raw, name, ...(pcName ? { pc: pcName } : {}) })}`;
}

accountApp.get('/link', (c) =>
	showLink(c, c.req.query('r') ?? '', c.req.query('name'), c.req.query('pc'), false)
);

accountApp.post('/link', async (c) => {
	const form = await c.req.parseBody();
	return showLink(
		c,
		formString(form, 'r') ?? '',
		formString(form, 'name'),
		formString(form, 'pc'),
		true
	);
});

/**
 * 結ぶ画面。`commit` が false なら確かめの画面を出し、true なら結んで返しのコードを出す。
 * 同じ申し込みをもう通していれば、どちらでも返しのコードを出し直す。
 */
async function showLink(
	c: Context<App>,
	raw: string,
	rawName: string | undefined,
	rawPcName: string | undefined,
	commit: boolean
) {
	const lang = resolveLang(c);
	const t = messages[lang];
	const name = cleanName(rawName) || DEFAULT_NAME;
	const pcName = cleanName(rawPcName);
	const next = linkUrl(raw, name, pcName);
	const account = await currentAccount(c);
	if (!account) return c.html(signIn(c, lang, next), commit ? 401 : 200);
	if (await limited(c, c.env.LINK_LIMITER, account.id)) {
		return c.html(messagePage(lang, t.tooManyTitle, t.tooManyLink), 429);
	}
	const at = now();
	const target = await linkTarget(c.env, account, raw, at);
	if (target.kind === 'invalid')
		return c.html(messagePage(lang, t.linkInvalidTitle, t.linkInvalid), 400);
	if (target.kind === 'other_account') {
		return c.html(messagePage(lang, t.linkTitle, t.linkOtherAccount), 409);
	}
	const active = await activePlan(c.env, account.id, at);
	if (!active)
		return c.html(noProPage(lang, account.email, next, stripeConfig(c.env) !== undefined));
	const limit = PLAN_LIMITS[active.plan];
	let list = await installationsOf(c.env, account.id, limit, at);
	const atLimit = () =>
		c.html(linkAtLimitPage(lang, account.email, limit, installationRows(list), next));
	if (target.kind === 'existing') {
		if (list.find((i) => i.row.id === target.row.id)?.overLimit) return atLimit();
		return c.html(
			linkedPage(lang, await issueCode(c.env, target.row.id, target.secret, active, at))
		);
	}
	const relink = target.previous !== undefined;
	if (!relink && list.length >= limit) return atLimit();
	if (!commit) {
		return c.html(
			linkPage(lang, account.email, {
				request: raw,
				name,
				pcName,
				relink,
				count: list.length,
				limit,
				next
			})
		);
	}
	if (target.previous) {
		// 同じ枠のまま、秘密 (公開鍵) と id を入れ替える。外していた行なら、結び直しで戻す。
		await c.env.DB.prepare(
			`UPDATE installations SET id = ?1, link_kid = ?2, public_key = ?3, name = ?4,
			   pc_name = CASE WHEN ?5 = '' THEN pc_name ELSE ?5 END, removed_at = NULL
			 WHERE id = ?6 AND account_id = ?7`
		)
			.bind(target.id, target.kid, target.publicKey, name, pcName, target.previous.id, account.id)
			.run();
	} else {
		// 数えるのと足すのを1つの文にする。別々だと、同時に結ばれたときに上限を超える。
		const inserted = await c.env.DB.prepare(
			`INSERT INTO installations (id, account_id, link_kid, public_key, name, pc_name, created_at)
			 SELECT ?1, ?2, ?3, ?4, ?5, ?8, ?6
			 WHERE (SELECT count(*) FROM installations WHERE account_id = ?2) < ?7
			 ON CONFLICT DO NOTHING`
		)
			.bind(target.id, account.id, target.kid, target.publicKey, name, at, limit, pcName)
			.run();
		if (inserted.meta.changes !== 1) {
			list = await installationsOf(c.env, account.id, limit, at);
			return atLimit();
		}
	}
	const code = await issueCode(c.env, target.id, target.secret, active, at);
	// ネットにつながらない WebLAV の前にいる人が、スマートフォンから離れても見られるように。送れなくても結んだことは変わらない。
	await sendMail(
		c.env,
		account.email,
		t.linkedCodeMailSubject,
		t.linkedCodeMailBody(installationName(name, pcName), code)
	).catch((e) => console.error('failed to send the link code', e));
	return c.html(linkedPage(lang, code));
}

/** 返しのコードを作り、出した許可の期限を覚える。 */
async function issueCode(
	env: Env,
	id: string,
	secret: Uint8Array,
	active: { plan: Plan; expiresAt: number },
	at: number
) {
	const code = await grantCode(secret, active.plan, at, active.expiresAt);
	await recordPermission(env, id, grantExpiresAt(at, active.expiresAt));
	return code;
}

// ---- 外した証し (→ docs/pro.md「外す」) ----

// 開いただけでは空けない。メールのリンクと同じく、先に開かれて使い切られないように。サインインは要らない (証しが本人の印)。
accountApp.get('/release', (c) => c.html(releasePage(resolveLang(c), c.req.query('c') ?? '')));

accountApp.post('/release', async (c) => {
	const lang = resolveLang(c);
	const t = messages[lang];
	if (await limited(c, c.env.INSTALLATION_LIMITER)) {
		return c.html(messagePage(lang, t.tooManyTitle, t.tooManyRelease), 429);
	}
	const form = await c.req.parseBody();
	const code = formString(form, 'c') ?? '';
	if (!parseRelease(code)) return c.html(messagePage(lang, t.releaseTitle, t.releaseInvalid), 400);
	// 知らない・もう外した証しも、同じく「空けました」と出す (2度目は何もしない)。
	await release(c.env, code);
	return c.html(messagePage(lang, t.releasedTitle, t.released));
});

/** アカウントのページから外す。WebLAV が受け取るか許可の期限が過ぎるまで、台数に数える。 */
accountApp.post('/installations/remove', async (c) => {
	const lang = resolveLang(c);
	const form = await c.req.parseBody();
	const next = safeNext(formString(form, 'next'));
	const account = await currentAccount(c);
	if (!account) return c.html(signIn(c, lang, next), 401);
	const at = now();
	await c.env.DB.batch([
		c.env.DB.prepare(
			'UPDATE installations SET removed_at = ? WHERE id = ? AND account_id = ? AND removed_at IS NULL'
		).bind(at, formString(form, 'id') ?? '', account.id),
		// 許可を持っていない (出していない・期限切れ) なら、すぐ消して枠を空ける。
		purgeRemoved(c.env, account.id, at)
	]);
	return c.redirect(next, 303);
});

/** カスタマーポータル (解約・支払い方法・領収書) へ。 */
accountApp.post('/billing', async (c) => {
	const lang = resolveLang(c);
	const account = await currentAccount(c);
	if (!account) return c.html(signIn(c, lang, ACCOUNT_HOME), 401);
	const config = stripeConfig(c.env);
	const row = await c.env.DB.prepare(
		`SELECT stripe_customer_id FROM subscriptions
		 WHERE account_id = ? AND stripe_customer_id IS NOT NULL ORDER BY paid_through DESC`
	)
		.bind(account.id)
		.first<{ stripe_customer_id: string }>();
	if (!config || !row) return c.redirect(ACCOUNT_HOME, 303);
	return c.redirect(
		await billingPortalUrl(config, row.stripe_customer_id, `${originOf(c)}${ACCOUNT_HOME}`, lang),
		303
	);
});

// ---- 月額と年額を切り替える (→ docs/pro.md「売り方」) ----

/** 確かめの画面で見せた今日の支払いを、そのまま使ってよい間。過ぎたら計算し直して見せ直す。 */
const SWITCH_QUOTE_TTL = 60 * 60;

/** 切り替えられる個人向けのサブスクの id。払い終えた期間が最も遅いもの。 */
async function switchableSubscription(env: Env, accountId: string): Promise<string | undefined> {
	const at = now();
	return (await subscriptionsOf(env, accountId)).find((r) => switchable(r, at))?.id;
}

function saleRegionOf(sub: Subscription): SaleRegion {
	return managedPaymentsOf(sub) ? 'overseas' : 'domestic';
}

/** 今のサブスクの状態から、切り替えの画面の中身を決める。切り替えられない形なら `undefined`。 */
async function planSwitchView(
	config: StripeConfig,
	lang: Lang,
	sub: Subscription
): Promise<PlanSwitch | undefined> {
	const interval = intervalOf(config, sub);
	if (!interval) return undefined;
	const periodEnd = sub.items.data[0].current_period_end;
	// 柔軟な請求 (billing_mode flexible) のサブスクは、ポータルで解約すると cancel_at_period_end でなく cancel_at が付く。
	if (sub.cancel_at_period_end || sub.cancel_at) {
		return { kind: 'canceled', until: sub.cancel_at ?? periodEnd };
	}
	const region = saleRegionOf(sub);
	if (interval === 'year') {
		return sub.schedule
			? { kind: 'reserved', switchAt: periodEnd }
			: { kind: 'toMonthly', region, switchAt: periodEnd };
	}
	const at = now();
	const preview = await previewYearlySwitch(config, sub, at);
	return {
		kind: 'toYearly',
		region,
		total: formatMoney(lang, preview.total, preview.currency),
		credit: formatMoney(lang, preview.credit, preview.currency),
		renewsAt: preview.periodEnd,
		at
	};
}

/** 切り替えの画面の2つの口が使う、今のサブスクとその画面の中身。切り替えられなければ `undefined`。 */
async function currentPlanSwitch(env: Env, accountId: string, lang: Lang) {
	const config = stripeConfig(env);
	const id = config && (await switchableSubscription(env, accountId));
	if (!id) return undefined;
	const sub = await getSubscription(config, id);
	const view = await planSwitchView(config, lang, sub);
	return view && { config, sub, view };
}

accountApp.get('/plan', async (c) => {
	const lang = resolveLang(c);
	const t = messages[lang];
	const account = await currentAccount(c);
	if (!account) return c.html(signIn(c, lang, PLAN_PATH));
	const current = await currentPlanSwitch(c.env, account.id, lang);
	if (!current) {
		return c.html(messagePage(lang, t.planSwitchTitle, t.planSwitchUnavailable, true), 404);
	}
	return c.html(planSwitchPage(lang, account.email, current.view));
});

// 画面を開いた後にほかのタブで切り替えていたら、今の状態の画面を出し直す (押した操作はしない)。
accountApp.post('/plan', async (c) => {
	const lang = resolveLang(c);
	const t = messages[lang];
	const account = await currentAccount(c);
	if (!account) return c.html(signIn(c, lang, PLAN_PATH), 401);
	const current = await currentPlanSwitch(c.env, account.id, lang);
	if (!current) {
		return c.html(messagePage(lang, t.planSwitchTitle, t.planSwitchUnavailable, true), 404);
	}
	const { config, sub, view } = current;
	const form = await c.req.parseBody();
	const action = formString(form, 'action');
	if (action === 'year' && view.kind === 'toYearly') {
		const quoted = Number(formString(form, 'at'));
		// 見せた額のまま払わせる。古い (または作った) 時刻なら、計算し直した額を見せて押し直してもらう。
		if (!(quoted <= view.at && view.at - quoted <= SWITCH_QUOTE_TTL)) {
			return c.html(planSwitchPage(lang, account.email, view, t.switchExpired), 409);
		}
		// 月額へ切り替わった直後で、スケジュールがまだ付いていると、次の区切りで上書きされる。
		if (sub.schedule) await releaseSchedule(config, sub.schedule);
		if (!(await switchToYearly(config, sub, quoted))) {
			return c.html(planSwitchPage(lang, account.email, view, t.switchFailed), 402);
		}
		// 払い終えた期間は、支払いの知らせ (invoice.paid) で延びる。
		const message = t.switchedYearly(formatDate(lang, view.renewsAt));
		return c.html(messagePage(lang, t.switchedYearlyTitle, message, true));
	}
	if (action === 'month' && view.kind === 'toMonthly') {
		await scheduleMonthly(config, sub);
		const message = t.planReserved(formatDate(lang, view.switchAt));
		return c.html(messagePage(lang, t.scheduledMonthlyTitle, message, true));
	}
	if (action === 'release' && view.kind === 'reserved') {
		await releaseSchedule(config, sub.schedule!);
		return c.html(messagePage(lang, t.switchReleasedTitle, t.switchReleased, true));
	}
	return c.html(planSwitchPage(lang, account.email, view), 409);
});

// ---- 個人向けの Pro を申し込む (→ docs/pro.md「売り方」) ----

/** アクセス元の IP の国 (Cloudflare が付ける)。手元で動かすときなど、分からなければ `undefined`。 */
function buyerCountry(c: Context<App>): string | undefined {
	return c.req.raw.cf?.country as string | undefined;
}

/** 最終確認の画面で、どちらの売り方の説明を出すか。売っていなければ `undefined`。 */
function saleRegion(c: Context<App>): SaleRegion | undefined {
	if (!stripeConfig(c.env)) return undefined;
	return usesManagedPayments(buyerCountry(c)) ? 'overseas' : 'domestic';
}

/** 料金ページで選んだプランの最終確認の画面。サインインしていなければ、サインインしてからこの画面へ戻す。 */
accountApp.get('/buy', async (c) => {
	const lang = resolveLang(c);
	const t = messages[lang];
	const plan = c.req.query('plan');
	if (plan !== 'month' && plan !== 'year') return c.redirect(PRICING_PATH, 303);
	const next = safeNext(c.req.query('next'));
	const account = await currentAccount(c);
	if (!account) {
		return c.html(signIn(c, lang, `${ACCOUNT}/buy?${new URLSearchParams({ plan, next })}`));
	}
	const region = saleRegion(c);
	if (!region) return c.html(messagePage(lang, t.buyTitle, t.notForSale), 404);
	// 持っているのに申し込ませない。料金ページから来た人には、黙って戻さず理由を出す。
	if (await activePlan(c.env, account.id, now())) return c.html(alreadyProPage(lang, next));
	return c.html(confirmPage(lang, account.email, { interval: plan, region, next }));
});

/** 最終確認の画面から、支払いの画面へ送る。済んだら `next` (結ぶ画面など) へ戻す。 */
accountApp.post('/buy', async (c) => {
	const lang = resolveLang(c);
	const t = messages[lang];
	const form = await c.req.parseBody();
	const next = safeNext(formString(form, 'next'));
	const interval: Interval = formString(form, 'interval') === 'year' ? 'year' : 'month';
	const account = await currentAccount(c);
	if (!account) return c.html(signIn(c, lang, next), 401);
	const config = stripeConfig(c.env);
	if (!config) return c.html(messagePage(lang, t.buyTitle, t.notForSale), 404);
	// 持っているのに申し込ませない。
	if (await activePlan(c.env, account.id, now())) return c.redirect(next, 303);
	// 支払いの画面の予約を取る。開いている画面があれば同じ画面へ送り、2つのタブや、確かめを待つ間の申し込み直しで
	// 二重に払わせないように。払い終えた画面を開き直すと、Stripe が払い終えたことを示す。
	const at = now();
	const reserved = await c.env.DB.prepare(
		'SELECT id, price, session_id FROM checkouts WHERE account_id = ? AND expires_at > ?'
	)
		.bind(account.id, at)
		.first<{ id: string; price: string; session_id: string | null }>();
	// 別の払い方で押し直したら、前の画面を閉じてから作り直す。
	if (reserved && reserved.price !== interval) {
		if (reserved.session_id) await expireCheckoutSession(config, reserved.session_id);
		await c.env.DB.prepare('DELETE FROM checkouts WHERE id = ?').bind(reserved.id).run();
	}
	await c.env.DB.batch([
		c.env.DB.prepare('DELETE FROM checkouts WHERE expires_at <= ?').bind(at),
		c.env.DB.prepare(
			`INSERT INTO checkouts (id, account_id, next, lang, expires_at, managed_payments, price)
			 VALUES (?, ?, ?, ?, ?, ?, ?)
			 ON CONFLICT (account_id) DO NOTHING`
		).bind(
			randomHex(16),
			account.id,
			next,
			lang,
			at + CHECKOUT_TTL,
			usesManagedPayments(buyerCountry(c)) ? 1 : 0,
			interval
		)
	]);
	const checkout = (await c.env.DB.prepare(
		'SELECT id, next, lang, url, expires_at, managed_payments, price FROM checkouts WHERE account_id = ?'
	)
		.bind(account.id)
		.first<{
			id: string;
			next: string;
			lang: Lang;
			url: string | null;
			expires_at: number;
			managed_payments: number;
			price: Interval;
		}>())!;
	const managedPayments = checkout.managed_payments === 1;
	if (checkout.url) return c.redirect(checkout.url, 303);
	// 画面がまだ無い予約 (ほかのタブが頼んでいる最中か、頼んだあとに失敗した) は、同じキーで頼み直す。
	// Stripe は作り終えた Session をそのまま返すので、画面は1つのまま。
	const origin = originOf(c);
	const done = new URLSearchParams({ next: checkout.next, lang: checkout.lang });
	let session: { id: string; url: string };
	try {
		session = await createCheckoutSession(config, {
			accountId: account.id,
			email: account.email,
			interval: checkout.price,
			lang: checkout.lang,
			successUrl: `${origin}${ACCOUNT}/buy/done?${done}`,
			cancelUrl: `${origin}${checkout.next}`,
			expiresAt: checkout.expires_at,
			submitMessage: managedPayments
				? undefined
				: messages[checkout.lang].checkoutNote(LEGAL_PAGES.tokushoho),
			managedPayments,
			idempotencyKey: `weblav-checkout-${checkout.id}`
		});
	} catch (e) {
		// 同じ予約をほかのタブが頼んでいる最中。予約は残し、押し直してもらう。
		if (e instanceof StripeError && e.status === 409) {
			return c.html(messagePage(lang, t.buyTitle, t.buyBusy), 409);
		}
		// 応答が届かなかったときは、Stripe が作ったかどうか分からない。予約を残し、押し直したら同じキーで頼み直す。
		if (!(e instanceof StripeError)) throw e;
		// Stripe が断ったときは予約を外し、押し直したら新しく頼めるように (同じキーでは、Stripe は同じ失敗を返し続ける)。
		await c.env.DB.prepare('DELETE FROM checkouts WHERE id = ? AND url IS NULL')
			.bind(checkout.id)
			.run();
		throw e;
	}
	await c.env.DB.prepare('UPDATE checkouts SET url = ?, session_id = ? WHERE id = ?')
		.bind(session.url, session.id, checkout.id)
		.run();
	return c.redirect(session.url, 303);
});

/** 支払いから戻った先。webhook が届いて Pro が付くのを待ち、付いたら `next` へ進む。 */
accountApp.get('/buy/done', async (c) => {
	const lang = resolveLang(c);
	const next = safeNext(c.req.query('next'));
	const account = await currentAccount(c);
	if (!account) return c.html(signIn(c, lang, next));
	if (await activePlan(c.env, account.id, now())) return c.redirect(next, 303);
	const tries = Number(c.req.query('tries') ?? '0') || 0;
	const retry = new URL(c.req.url);
	retry.searchParams.set('tries', String(tries + 1));
	return c.html(
		checkingPurchasePage(lang, `${retry.pathname}${retry.search}`, tries < PURCHASE_CHECKS)
	);
});

// ---- Pro を別のアカウントへ移す (→ docs/pro.md「アカウントと販売の窓口」) ----

accountApp.get('/transfer', async (c) => {
	const lang = resolveLang(c);
	const t = messages[lang];
	const account = await currentAccount(c);
	if (!account) return c.html(signIn(c, lang, TRANSFER_PATH));
	if ((await accountPlans(c.env, account)).length === 0) {
		return c.html(messagePage(lang, t.transferTitle, t.noPlans), 403);
	}
	return c.html(transferPage(lang, account.email));
});

// 1回目の送信では移さず、移し先を見せて確かめる。打ち間違えたアドレスへ移さないように。
accountApp.post('/transfer', async (c) => {
	const lang = resolveLang(c);
	const t = messages[lang];
	const account = await currentAccount(c);
	if (!account) return c.html(signIn(c, lang, TRANSFER_PATH), 401);
	const plans = await accountPlans(c.env, account);
	if (plans.length === 0) return c.html(messagePage(lang, t.transferTitle, t.noPlans), 403);
	const form = await c.req.parseBody();
	const to = normalizeEmail(formString(form, 'email') ?? '');
	if (!isEmail(to)) return c.html(transferPage(lang, account.email, t.invalidEmail), 400);
	if (to === account.email) return c.html(transferPage(lang, account.email, t.transferToSelf), 400);
	if (formString(form, 'confirm') !== '1') {
		return c.html(transferConfirmPage(lang, account.email, to, plans));
	}
	// 移し先と元の両方へメールを送るので、リンクと同じ枠で絞る。
	if (await limited(c, c.env.EMAIL_LIMITER)) {
		return c.html(transferPage(lang, account.email, t.tooManyTransfers), 429);
	}
	const moved = await transferPro(c.env, account, to);
	if (moved === 0) return c.html(messagePage(lang, t.transferTitle, t.noPlans), 403);
	// 移した後は取り消せないので、知らせが送れなくても失敗にはしない。
	// 1通が失敗しても、もう1通は送り切る。
	const sent = await Promise.allSettled([
		sendMail(c.env, account.email, t.transferredFromSubject, t.transferredFromBody(to)),
		sendMail(
			c.env,
			to,
			t.transferredToSubject,
			t.transferredToBody(account.email, `${originOf(c)}${ACCOUNT_HOME}?lang=${lang}`)
		)
	]);
	const failed = sent.filter((r) => r.status === 'rejected');
	for (const r of failed) console.error('failed to send transfer notice', r.reason);
	return c.html(transferredPage(lang, to, failed.length === 0));
});

/**
 * 持っている Pro (サブスクの行) と結んでいる WebLAV をすべて移し先へ付け替え、移した Pro の数を返す。
 * 移し先のアカウントが無ければ作る。Stripe の Customer は変えない (移し先がカスタマーポータルで直す)。
 */
async function transferPro(env: Env, from: Account, to: string): Promise<number> {
	const target = '(SELECT id FROM accounts WHERE email = ?)';
	const [, moved] = await env.DB.batch([
		ensureAccount(env, to, now()),
		env.DB.prepare(`UPDATE subscriptions SET account_id = ${target} WHERE account_id = ?`).bind(
			to,
			from.id
		),
		env.DB.prepare(`UPDATE installations SET account_id = ${target} WHERE account_id = ?`).bind(
			to,
			from.id
		)
	]);
	return moved.meta.changes;
}

/** 移す画面に出す、持っている Pro の種類。 */
async function accountPlans(env: Env, account: Account): Promise<Plan[]> {
	const at = now();
	const rows = await subscriptionsOf(env, account.id);
	return [...new Set(rows.filter((r) => isLive(r, at)).map((r) => r.plan))];
}

app.route(ACCOUNT, accountApp);

export default {
	fetch: app.fetch
} satisfies ExportedHandler<Env>;
