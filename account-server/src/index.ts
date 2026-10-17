/**
 * WebLAV の窓口 (→ docs/account-server.md「窓口（weblav.amiiby.com）」)。
 *
 * - `/v1/links/token`: PC の登録を窓口の画面で認めたかを WebLAV が問い合わせ、認めていればアプリ用のトークンに替える。
 * - `/v1/ai`・`/v1/search`・`/v1/balance`・`/v1/token`: アプリ用のトークンで呼ぶ、AI と検索の中継・残高・登録の解除。
 * - `/v1/stripe/webhook`: Stripe からの支払いの知らせ。
 * - `/account/`: 人が開く画面 (サインイン・PC の登録・買う)。
 * - それ以外: 紹介と規約類の静的なページ (wrangler.jsonc の assets)。
 */
import { Hono, type Context } from 'hono';
import { csrf } from 'hono/csrf';
import { inputCostEstimate, MAX_BODY_BYTES, parsePrompt, relay } from './ai';
import { balance, charge, grantFreeStatement } from './credits';
import { messages, resolveLang, type Lang } from './i18n';
import { pricing, usedShare } from './pricing';
import { closeSale } from './sale';
import { parseSearch, search, searchCost } from './search';
import { sendMail } from './mail';
import {
	approvePage,
	checkingPurchasePage,
	confirmPage,
	confirmSignInPage,
	homePage,
	LEGAL_PAGES,
	mailSentPage,
	messagePage,
	signInPage,
	type LinkedApp,
	type LinkRequest,
	type SaleRegion
} from './pages';
import { appleConfig, finishAppleSignIn, startAppleSignIn } from './apple';
import { finishGoogleSignIn, googleConfig, startGoogleSignIn } from './google';
import {
	confirmPurchase,
	createCheckoutSession,
	PRODUCT,
	StripeError,
	stripeConfig,
	type StripeConfig,
	usesManagedPayments,
	verifyWebhook
} from './stripe';
import { currentAccount, endSession, startSession } from './session';
import {
	isEmail,
	normalizeEmail,
	formString,
	now,
	randomHex,
	safeNext,
	ACCOUNT,
	ACCOUNT_HOME,
	sha256Hex
} from './util';

/** 窓口の画面で認めてから、WebLAV がトークンに替えるまでの期限。WebLAV は数秒おきに問い合わせるので、通信が少し途切れても間に合う長さ。 */
const LINK_APPROVAL_TTL = 600;
/** メールのリンクの期限。メールが少し遅れて届いても間に合い、漏れたリンクを使える時間は短く。 */
const EMAIL_LOGIN_TTL = 15 * 60;
/** 同じアドレスへ1時間に送るリンクの上限。他人のアドレスへ送りつけるのに使われないように。 */
const EMAIL_LOGINS_PER_HOUR = 5;
/** 上の上限を数える幅。上限を「1時間に何通」で決めているので、その1時間。 */
const EMAIL_LOGIN_WINDOW = 3600;
/** 支払いから戻った先で、残高が付くのを自動で確かめ直す回数 (3秒おき)。webhook は数秒で届くことが多い。 */
const PURCHASE_CHECKS = 10;
/**
 * 支払いの画面の期限。Stripe が受け付ける最も短い30分に、同じ予約で頼み直せる5分を足す
 * (Stripe は頼むたびに期限が30分以上先かを確かめる)。
 */
const CHECKOUT_TTL = 35 * 60;
/**
 * アカウントを消したあと、購入の記録を残す期間。適格請求書の写しと帳簿の保存期間 (7年) に、うるう年の分を足す
 * (3つのアプリで揃える決まり)。使った分の記録も同じ期間残す。
 */
const PURCHASE_RETENTION = 7 * 366 * 24 * 60 * 60;

type App = { Bindings: Env };
// `/account` と `/account/` を同じ画面にする (紹介のページは末尾に `/` を付けてリンクする)。
const app = new Hono<App>({ strict: false });

// 値付けの値が欠けていれば、どの入口も DB に書く前に止める (→ docs/account-server.md「値付けの値」)。
// サインインのリンクを使ってから 500 になるような、やり直せない途中で止まらないように。
app.use(async (c, next) => {
	pricing(c.env);
	await next();
});

/**
 * 上限に当たったら true。既定は送り主 (IP) ごとに数え、`key` を渡せばそれごと (アカウントごとなど) に数える。
 * D1 の書き込みの枠やメールを使い切られないように。
 * 数え方はおおよそ (Workers の Rate Limiting)。正確さより、止めることを取る。
 */
async function limited(c: Context<App>, limiter: RateLimit, key?: string): Promise<boolean> {
	const { success } = await limiter.limit({
		key: key ?? c.req.header('cf-connecting-ip') ?? 'unknown'
	});
	return !success;
}

// ---- PC を登録する ----

/**
 * PC の登録を窓口の画面で認めたかを、WebLAV が問い合わせる。認めていれば、アプリ用のトークンに替える。
 * 申し込みは WebLAV が作った検証用の値のハッシュで見分け、検証用の値そのものを持つ WebLAV だけが替えられる
 * (RFC 7636 の PKCE と同じ考え方)。申し込みの URL を見た人には、トークンを取れない。
 */
app.post('/v1/links/token', async (c) => {
	if (await limited(c, c.env.LINK_LIMITER)) return c.json({ error: 'rate_limited' }, 429);
	const body = await c.req.json<{ code_verifier?: unknown }>().catch(() => ({}) as never);
	if (typeof body.code_verifier !== 'string' || !/^[0-9a-f]{64}$/.test(body.code_verifier)) {
		return c.json({ error: 'invalid_request' }, 400);
	}
	const challenge = await sha256Hex(body.code_verifier);
	const token = randomHex(32);
	const t = now();
	// トークンを作るのと認めた印を消すのを、1つのまとまりで流す。片方だけが済んで、認めた印だけが消えることがないように。
	const [created] = await c.env.DB.batch([
		c.env.DB.prepare(
			`INSERT INTO app_tokens (id, token_hash, account_id, name, pc_name, created_at)
			 SELECT ?1, ?2, account_id, name, pc_name, ?3 FROM link_approvals
			 WHERE challenge = ?4 AND expires_at > ?3`
		).bind(randomHex(16), await sha256Hex(token), t, challenge),
		c.env.DB.prepare('DELETE FROM link_approvals WHERE challenge = ?').bind(challenge)
	]);
	// まだ認めていない (か、認めてから期限が過ぎた)。WebLAV は間を置いて問い合わせ直す。
	if (created.meta.changes !== 1) return c.json({ status: 'pending' }, 202);
	return c.json({ token });
});

// ---- アプリ用のトークンで呼ぶもの ----

/** `Authorization: Bearer <トークン>` のトークン。 */
function bearerToken(c: Context<App>): string | undefined {
	return /^Bearer (\S+)$/.exec(c.req.header('authorization') ?? '')?.[1];
}

/** アプリ用のトークンのアカウント。トークンが無いか、外されていれば `undefined`。 */
async function appAccount(c: Context<App>): Promise<string | undefined> {
	const token = bearerToken(c);
	if (!token) return undefined;
	const row = await c.env.DB.prepare(
		'UPDATE app_tokens SET last_used_at = ? WHERE token_hash = ? RETURNING account_id'
	)
		.bind(now(), await sha256Hex(token))
		.first<{ account_id: string }>();
	return row?.account_id;
}

/** WebLAV の設定に出す残り。 */
app.get('/v1/balance', async (c) => {
	const accountId = await appAccount(c);
	if (!accountId) return c.json({ error: 'unauthorized' }, 401);
	const account = await c.env.DB.prepare('SELECT email FROM accounts WHERE id = ?')
		.bind(accountId)
		.first<{ email: string }>();
	// 無料の分を取りこぼしていれば、ここで付け直す (→ src/credits.ts)。
	await grantFreeStatement(c.env, accountId).run();
	const { remaining, percent } = await balance(c.env, accountId);
	return c.json({ email: account?.email, remaining_percent: remaining > 0 ? percent : 0 });
});

/** WebLAV での登録の解除。トークンを外す。 */
app.delete('/v1/token', async (c) => {
	const token = bearerToken(c);
	if (token) {
		await c.env.DB.prepare('DELETE FROM app_tokens WHERE token_hash = ?')
			.bind(await sha256Hex(token))
			.run();
	}
	return c.body(null, 204);
});

/** 使った量を WebLAV へ知らせるヘッダー (→ src/pricing.ts の usedShare)。 */
const USED_HEADER = 'x-weblav-used';

/** 同じアカウントで同時に受ける呼び出しの数。登録した PC の人たちが重ねて使えて、残りを超えて使われる分を小さく抑える数。 */
const MAX_IN_FLIGHT = 3;
/**
 * 呼び出しの最中とみなす長さ。Gemini を待つ上限 (src/ai.ts) より長くする。
 * Worker が途中で止まって印が残っても、これを過ぎれば数えない。
 */
const IN_FLIGHT_TTL = 120;

/**
 * 残高を使う入口に共通の前段。トークンと呼びすぎを確かめ、通ればアカウントを返す。
 */
async function spender(c: Context<App>): Promise<string | Response> {
	const accountId = await appAccount(c);
	if (!accountId) return c.json({ error: 'unauthorized' }, 401);
	if (await limited(c, c.env.AI_LIMITER, accountId)) return c.json({ error: 'rate_limited' }, 429);
	// 無料の分を取りこぼしていれば、ここで付け直す (→ src/credits.ts)。
	await grantFreeStatement(c.env, accountId).run();
	return accountId;
}

/**
 * 残りを確かめてから呼び、使った原価を引く (→ docs/account-server.md「中継」)。
 * - 残りが `needed` (入力の原価の見積もり) に届かなければ、呼ばずに 402。
 * - 同じアカウントの呼び出しは `MAX_IN_FLIGHT` 件まで重ねて受ける。登録した PC とその利用者が残高を共有するため。
 *   残りを超えて使われる分は、重なった数 × 1回の出力の原価までに収まる。
 * - WebLAV で取り消して接続が切れても、使った分を引き、印を外すまで続けさせる
 *   (呼び出しが終わると、waitUntil に渡していない処理は打ち切られうる)。
 * - 業者に届かなかった (投げた) 回は引かず、`undefined` を返す。
 */
async function spend<T>(
	c: Context<App>,
	accountId: string,
	needed: number,
	call: () => Promise<T>,
	cost: (result: T) => number
): Promise<T | undefined | Response> {
	const t = now();
	const owner = randomHex(16);
	const [, started] = await c.env.DB.batch([
		c.env.DB.prepare('DELETE FROM ai_in_flight WHERE account_id = ? AND started_at <= ?').bind(
			accountId,
			t - IN_FLIGHT_TTL
		),
		c.env.DB.prepare(
			`INSERT INTO ai_in_flight (id, account_id, started_at)
			 SELECT ?1, ?2, ?3 WHERE (SELECT count(*) FROM ai_in_flight WHERE account_id = ?2) < ?4`
		).bind(owner, accountId, t, MAX_IN_FLIGHT)
	]);
	if (started.meta.changes !== 1) return c.json({ error: 'rate_limited' }, 429);
	const release = () => c.env.DB.prepare('DELETE FROM ai_in_flight WHERE id = ?').bind(owner).run();
	const { remaining } = await balance(c.env, accountId);
	if (remaining <= 0 || remaining < needed) {
		await release();
		return c.json({ error: 'no_credit' }, 402);
	}
	const work = (async () => {
		try {
			const result = await call();
			await charge(c.env, accountId, cost(result));
			return result;
		} catch (error) {
			// 中身を含みうるので、種類だけを残す。
			console.error('Upstream call failed', error instanceof Error ? error.name : 'unknown');
			return undefined;
		} finally {
			await release();
		}
	})();
	c.executionCtx.waitUntil(work);
	return work;
}

/**
 * AI の中継。残りがあれば Gemini へ送り、返事が届いたら使った原価を引いて、Gemini の返事をそのまま返す。
 * 返事が届かなかった回は引かない。失敗の返事でも、Gemini が使った分を返せば引く。
 */
app.post('/v1/ai', async (c) => {
	const accountId = await spender(c);
	if (typeof accountId !== 'string') return accountId;
	// 大きすぎる本文は、読む前に断る (Worker のメモリを使わせない)。
	if (Number(c.req.header('content-length') ?? 0) > MAX_BODY_BYTES) {
		return c.json({ error: 'too_long' }, 413);
	}
	const prompt = parsePrompt(await c.req.json().catch(() => undefined));
	if (prompt === undefined) return c.json({ error: 'bad_request' }, 400);
	if (prompt === 'too_long') return c.json({ error: 'too_long' }, 413);
	const { rates } = pricing(c.env);
	const result = await spend(
		c,
		accountId,
		inputCostEstimate(prompt, rates),
		() => relay(c.env, prompt, rates),
		(r) => r.cost
	);
	if (result instanceof Response) return result;
	// Gemini に届かなかった。失敗の返事と同じ形で返す (WebLAV は AI の側の失敗として出す)。
	if (result === undefined) return c.json({ error: 'upstream' }, 502);
	const used = { [USED_HEADER]: String(usedShare(c.env, result.cost)) };
	if (result.ok) return c.body(result.body, 200, { 'content-type': 'application/json', ...used });
	// Gemini の失敗は、状態だけを WebLAV へ返す。
	return c.json({ error: 'upstream', upstream_status: result.status }, 502, used);
});

/**
 * Web の検索の中継。AI が集めるリンクの候補を引く。成功した回だけ、1回分の原価を引く。
 */
app.post('/v1/search', async (c) => {
	const accountId = await spender(c);
	if (typeof accountId !== 'string') return accountId;
	const request = parseSearch(await c.req.json().catch(() => undefined));
	if (request === undefined) return c.json({ error: 'bad_request' }, 400);
	const { rates } = pricing(c.env);
	const result = await spend(
		c,
		accountId,
		searchCost(rates),
		() => search(c.env, request, rates),
		(r) => (r.ok ? r.cost : 0)
	);
	if (result instanceof Response) return result;
	if (result === undefined) return c.json({ error: 'upstream' }, 502);
	if (!result.ok) return c.json({ error: 'upstream', upstream_status: result.status }, 502);
	return c.json({ results: result.results }, 200, {
		[USED_HEADER]: String(usedShare(c.env, result.cost))
	});
});

// ---- Stripe からの知らせ ----

/**
 * 残高を付けるのはここだけ。支払いの画面から戻った先では付けない (戻らずに閉じられることもあるため)。
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
	try {
		await handleStripeEvent(c.env, config, event.type, event.data.object);
	} catch (e) {
		await markStripeEvent(c.env, event.id, 'failed');
		throw e;
	}
	await markStripeEvent(c.env, event.id, 'done');
	// 片付けは次の知らせのときにもやり直せるので、失敗しても知らせは受け取ったことにする。
	await forgetOldRecords(c.env).catch((e) => console.error('failed to forget old records', e));
	return c.json({ received: true });
});

/**
 * 保存の期間を過ぎた記録を消す。購入の台帳は、アカウントから外れて期間を過ぎたもの (期間は外した日時から数える。
 * アカウントが無いまま届いた購入は、購入の日時から)。使った分の記録は、購入の分は台帳と一緒に消え、無料の分は日時で消す。
 * 知らせは少ないので、受けるたびに片付ける。
 */
function forgetOldRecords(env: Env) {
	const before = now() - PURCHASE_RETENTION;
	return env.DB.batch([
		env.DB.prepare(
			`DELETE FROM purchases
			 WHERE account_id IS NULL AND coalesce(detached_at, created_at) <= ?
			   AND NOT EXISTS (SELECT 1 FROM grants WHERE purchase_id = purchases.id)`
		).bind(before),
		env.DB.prepare('DELETE FROM consumptions WHERE purchase_id IS NULL AND created_at <= ?').bind(
			before
		),
		env.DB.prepare('DELETE FROM stripe_revoked_payments WHERE created_at <= ?').bind(before)
	]);
}

function markStripeEvent(env: Env, id: string, status: 'done' | 'failed') {
	return env.DB.prepare('UPDATE stripe_events SET status = ?, updated_at = ? WHERE id = ?')
		.bind(status, now(), id)
		.run();
}

/** 同じ Stripe のアカウントのほかの製品の知らせも届くので、metadata の印で見分け、ほかは何もしない。 */
async function handleStripeEvent(
	env: Env,
	config: StripeConfig,
	type: string,
	object: Record<string, unknown>
) {
	switch (type) {
		// カードは completed の時点で払われている。MP の Session では後から払う方法も選べ、払われると async_payment_succeeded が届く。
		case 'checkout.session.completed':
		case 'checkout.session.async_payment_succeeded': {
			const metadata = object.metadata as Record<string, string> | null;
			if (metadata?.product !== PRODUCT || object.payment_status !== 'paid') return;
			const purchase = await confirmPurchase(config, String(object.id));
			if (!purchase) return;
			// 台帳に残してから、残高を付ける。1つのトランザクションにし、確かめる処理と付ける処理を分けない。
			// アカウントが無ければ、台帳には結び付き無しで残し、残高は付けない。
			// 返金・不審請求の知らせが先に届いていれば、取り消し済みとして残し、残高は付けない。
			const t = now();
			await env.DB.batch([
				env.DB.prepare(
					`INSERT INTO purchases
					   (id, account_id, product, stripe_checkout_session_id, stripe_payment_intent_id,
					    amount, currency, managed_payments, card_country, buyer_country, domestic, created_at,
					    revoked_at)
					 VALUES (?1, (SELECT id FROM accounts WHERE id = ?2), ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?12, ?11,
					   (SELECT created_at FROM stripe_revoked_payments WHERE payment_intent_id = ?5))
					 ON CONFLICT DO NOTHING`
				).bind(
					randomHex(16),
					purchase.accountId,
					PRODUCT,
					purchase.sessionId,
					purchase.paymentIntentId,
					purchase.amount,
					purchase.currency,
					purchase.managedPayments ? 1 : 0,
					purchase.cardCountry,
					purchase.buyerCountry,
					t,
					purchase.domestic ? 1 : 0
				),
				env.DB.prepare(
					`INSERT INTO grants (id, account_id, purchase_id, granted, remaining, created_at)
					 SELECT ?1, account_id, id, ?2, ?2, ?3 FROM purchases
					 WHERE stripe_checkout_session_id = ?4 AND account_id IS NOT NULL AND revoked_at IS NULL
					 ON CONFLICT DO NOTHING`
				).bind(randomHex(16), pricing(env).purchaseGrant, t, purchase.sessionId),
				// 支払いの画面の予約を外す。次の買い足しで、払い終えた画面へ送らないように。
				env.DB.prepare('DELETE FROM checkouts WHERE account_id = ?').bind(purchase.accountId)
			]);
			return;
		}
		// 全額を返金したとき・不審請求を申し立てられたときは、その購入の残りを取り消す。使った分は取り戻さない。
		case 'charge.refunded':
		case 'charge.dispute.created': {
			if (type === 'charge.refunded' && object.refunded !== true) return;
			if (typeof object.payment_intent !== 'string') return;
			// 付ける知らせが後から届いても付けないよう、取り消した支払いを覚えておく。台帳の行は消さない。
			const t = now();
			await env.DB.batch([
				env.DB.prepare(
					`INSERT INTO stripe_revoked_payments (payment_intent_id, created_at) VALUES (?, ?)
					 ON CONFLICT DO NOTHING`
				).bind(object.payment_intent, t),
				env.DB.prepare(
					`UPDATE purchases SET revoked_at = ?
					 WHERE stripe_payment_intent_id = ? AND revoked_at IS NULL`
				).bind(t, object.payment_intent),
				env.DB.prepare(
					`UPDATE grants SET revoked = revoked + remaining, remaining = 0 WHERE purchase_id IN
					   (SELECT id FROM purchases WHERE stripe_payment_intent_id = ?)`
				).bind(object.payment_intent)
			]);
			return;
		}
	}
}

// ---- 人が開く画面 ----

const accountApp = new Hono<App>();
accountApp.use(
	csrf({
		// Apple はサインインの結果を、Apple のサイトから戻り先へ POST で送る。送り元が `null` で届くこともあるので、
		// 戻り先だけは送り元を見ず、state の照らし合わせで守る (→ src/apple.ts)。
		origin: (origin, c) => origin === new URL(c.req.url).origin || c.req.path === APPLE_CALLBACK
	})
);

accountApp.get('/', async (c) => {
	const lang = resolveLang(c);
	const account = await currentAccount(c);
	if (!account) return c.html(signIn(c, lang, ACCOUNT_HOME));
	const { results: apps } = await c.env.DB.prepare(
		`SELECT id, name, pc_name AS pcName, created_at AS createdAt FROM app_tokens
		 WHERE account_id = ? ORDER BY created_at`
	)
		.bind(account.id)
		.all<LinkedApp>();
	return c.html(
		homePage(lang, account.email, await balance(c.env, account.id), apps, saleRegion(c), {
			bought: c.req.query('bought') === '1'
		})
	);
});

/** 窓口の画面から、PC の登録を外す。外した PC の WebLAV は、登録し直すまで AI の機能を使えない。 */
accountApp.post('/apps/unlink', async (c) => {
	const lang = resolveLang(c);
	const account = await currentAccount(c);
	if (!account) return c.html(signIn(c, lang, ACCOUNT_HOME), 401);
	const form = await c.req.parseBody();
	await c.env.DB.prepare('DELETE FROM app_tokens WHERE id = ? AND account_id = ?')
		.bind(formString(form, 'id') ?? '', account.id)
		.run();
	return c.redirect(ACCOUNT_HOME, 303);
});

accountApp.post('/login/email', async (c) => {
	const lang = resolveLang(c);
	const form = await c.req.parseBody();
	const next = safeNext(formString(form, 'next'));
	const email = normalizeEmail(formString(form, 'email') ?? '');
	if (!isEmail(email)) {
		return c.html(signIn(c, lang, next, messages[lang].invalidEmail), 400);
	}
	if (await limited(c, c.env.EMAIL_LIMITER)) {
		return c.html(signIn(c, lang, next, messages[lang].tooManyLinks), 429);
	}
	const t = now();
	const token = randomHex(32);
	// 数えるのと足すのを1つの文にする。別々だと、同時に送られたときに上限を超える。
	const [, inserted] = await c.env.DB.batch([
		c.env.DB.prepare('DELETE FROM email_logins WHERE created_at <= ?').bind(t - EMAIL_LOGIN_WINDOW),
		c.env.DB.prepare(
			`INSERT INTO email_logins (token_hash, email, next, expires_at, created_at)
			 SELECT ?1, ?2, ?3, ?4, ?5
			 WHERE (SELECT count(*) FROM email_logins WHERE email = ?2 AND created_at > ?6) < ?7`
		).bind(
			await sha256Hex(token),
			email,
			next,
			t + EMAIL_LOGIN_TTL,
			t,
			t - EMAIL_LOGIN_WINDOW,
			EMAIL_LOGINS_PER_HOUR
		)
	]);
	if (inserted.meta.changes === 0) {
		return c.html(signIn(c, lang, next, messages[lang].tooManyLinks), 429);
	}
	// 別のブラウザで開いても同じ言語になるよう、リンクに言語を付ける。
	await sendMail(
		c.env,
		email,
		messages[lang].mailSubject,
		messages[lang].mailBody(
			`${new URL(c.req.url).origin}${ACCOUNT}/login/email?token=${token}&lang=${lang}`,
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
	const form = await c.req.parseBody();
	const token = formString(form, 'token') ?? '';
	const t = now();
	const login = await c.env.DB.prepare(
		`UPDATE email_logins SET used_at = ?
		 WHERE token_hash = ? AND used_at IS NULL AND expires_at > ?
		 RETURNING email, next`
	)
		.bind(t, await sha256Hex(token), t)
		.first<{ email: string; next: string }>();
	if (!login) {
		return c.html(
			messagePage(lang, messages[lang].linkUnusableTitle, messages[lang].linkUnusable),
			400
		);
	}
	const account = await upsertAccount(c.env, login.email, t).first<{ id: string }>();
	await signedIn(c, account!.id);
	return c.redirect(safeNext(login.next), 303);
});

/** サインインが済んだ。無料の分をまだ付けていなければ、ここで付ける (→ src/credits.ts)。 */
async function signedIn(c: Context<App>, accountId: string) {
	await startSession(c, accountId);
	await grantFreeStatement(c.env, accountId).run();
}

// ---- Google でサインイン ----

accountApp.get('/login/google', async (c) => {
	const config = googleConfig(c.env);
	if (!config) return c.notFound();
	const next = safeNext(c.req.query('next'));
	return c.redirect(await startGoogleSignIn(c, config, googleRedirectUri(c), next), 303);
});

accountApp.get('/login/google/callback', async (c) => {
	const lang = resolveLang(c);
	const config = googleConfig(c.env);
	if (!config) return c.notFound();
	const user = await finishGoogleSignIn(c, config, googleRedirectUri(c), now());
	if ('failure' in user) {
		const error =
			user.failure === 'unconfirmed_email'
				? messages[lang].googleUnconfirmed
				: messages[lang].googleFailed;
		return c.html(signIn(c, lang, user.next, error), 400);
	}
	return externalSignedIn(c, lang, 'google', user, messages[lang].googleConflict);
});

function googleRedirectUri(c: Context<App>): string {
	return `${new URL(c.req.url).origin}${ACCOUNT}/login/google/callback`;
}

// ---- Apple でサインイン ----

const APPLE_CALLBACK = `${ACCOUNT}/login/apple/callback`;

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
	if ('failure' in user) {
		// 取り消したときは、何も言わずにサインインの画面へ戻す。
		return user.failure === 'cancelled'
			? c.html(signIn(c, lang, user.next))
			: c.html(signIn(c, lang, user.next, messages[lang].appleFailed), 400);
	}
	return externalSignedIn(c, lang, 'apple', user, messages[lang].appleConflict);
});

function appleRedirectUri(c: Context<App>): string {
	return `${new URL(c.req.url).origin}${APPLE_CALLBACK}`;
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
	await signedIn(c, accountId);
	return c.redirect(safeNext(user.next), 303);
}

/**
 * 外部のサインインのアカウントに結ぶ窓口のアカウント。確かめ済みのメールだけを渡す。
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
	const t = now();
	const [account] = await env.DB.batch<{ id: string }>([
		// メールのリンクと同じく、同じアドレスなら同じアカウント。
		upsertAccount(env, email, t),
		env.DB.prepare(
			`INSERT INTO identities (provider, subject, account_id, created_at)
			 SELECT ?1, ?2, id, ?3 FROM accounts
			 WHERE email = ?4
			   AND NOT EXISTS (SELECT 1 FROM identities
			                   WHERE provider = ?1 AND account_id = accounts.id)
			 ON CONFLICT DO NOTHING`
		).bind(provider, subject, t, email)
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
function upsertAccount(env: Env, email: string, t: number) {
	return env.DB.prepare(
		`INSERT INTO accounts (id, email, created_at) VALUES (?, ?, ?)
		 ON CONFLICT (email) DO UPDATE SET email = excluded.email
		 RETURNING id`
	).bind(randomHex(16), email, t);
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

// ---- PC を登録する ----

/** 名前は窓口の画面に出すだけ。制御文字を除き、長すぎれば切る。 */
function displayName(value: string | undefined, fallback: string): string {
	const name = [...(value ?? '').replace(/[\p{Cc}\p{Cf}]/gu, '').trim()].slice(0, 64).join('');
	return name || fallback;
}

/** WebLAV が申し込みに付けた値を読む。形が違えば `undefined` (WebLAV のサイト設定から開き直してもらう)。 */
function linkRequest(get: (key: string) => string | undefined): LinkRequest | undefined {
	const challenge = get('challenge') ?? '';
	if (!/^[0-9a-f]{64}$/.test(challenge)) return undefined;
	return {
		challenge,
		name: displayName(get('name'), 'WebLAV'),
		pcName: displayName(get('pc'), 'PC')
	};
}

function linkQuery(link: LinkRequest): string {
	return new URLSearchParams({
		challenge: link.challenge,
		name: link.name,
		pc: link.pcName
	}).toString();
}

accountApp.get('/link', async (c) => {
	const lang = resolveLang(c);
	const link = linkRequest((key) => c.req.query(key));
	if (!link) return c.html(linkInvalidPage(lang), 400);
	const account = await currentAccount(c);
	if (!account) return c.html(signIn(c, lang, `${ACCOUNT}/link?${linkQuery(link)}`));
	return c.html(approvePage(lang, account.email, link, `${ACCOUNT}/link?${linkQuery(link)}`));
});

/**
 * 登録を認める。認めた印を残し、WebLAV が検証用の値を持って問い合わせたときにトークンに替える。
 * 管理者は WebLAV を別の端末 (スマートフォンなど) から操作することがあるので、ブラウザを PC へ戻す形にはしない。
 */
accountApp.post('/link', async (c) => {
	const lang = resolveLang(c);
	const form = await c.req.parseBody();
	const link = linkRequest((key) => formString(form, key));
	if (!link) return c.html(linkInvalidPage(lang), 400);
	const account = await currentAccount(c);
	if (!account) return c.html(signIn(c, lang, `${ACCOUNT}/link?${linkQuery(link)}`), 401);
	if (await limited(c, c.env.CODE_LIMITER, account.id)) {
		return c.html(
			messagePage(lang, messages[lang].tooManyConnectsTitle, messages[lang].tooManyConnects),
			429
		);
	}
	const t = now();
	await c.env.DB.batch([
		c.env.DB.prepare('DELETE FROM link_approvals WHERE expires_at <= ?').bind(t),
		// 同じ申し込みを押し直しても、先に認めたアカウントのままにする。
		c.env.DB.prepare(
			`INSERT INTO link_approvals (challenge, account_id, name, pc_name, expires_at, created_at)
			 VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT (challenge) DO NOTHING`
		).bind(link.challenge, account.id, link.name, link.pcName, t + LINK_APPROVAL_TTL, t)
	]);
	return c.html(messagePage(lang, messages[lang].linkedTitle, messages[lang].linked(link.pcName)));
});

function linkInvalidPage(lang: Lang) {
	return messagePage(lang, messages[lang].linkInvalidTitle, messages[lang].linkInvalid);
}

// ---- 残高を買う ----

/** アクセス元の IP の国 (Cloudflare が付ける)。手元で動かすときなど、分からなければ `undefined`。 */
function buyerCountry(c: Context<App>): string | undefined {
	return c.req.raw.cf?.country as string | undefined;
}

/** 買うボタンを出すか、出すならどちらの売り方の説明を添えるか。 */
function saleRegion(c: Context<App>): SaleRegion | undefined {
	if (!stripeConfig(c.env)) return undefined;
	return usesManagedPayments(buyerCountry(c)) ? 'overseas' : 'domestic';
}

/** 料金ページから来る、支払いの直前の最終確認画面。 */
accountApp.get('/buy', async (c) => {
	const lang = resolveLang(c);
	const account = await currentAccount(c);
	if (!account) return c.html(signIn(c, lang, `${ACCOUNT}/buy`));
	const region = saleRegion(c);
	if (!region)
		return c.html(messagePage(lang, messages[lang].buyTitle, messages[lang].notForSale), 404);
	return c.html(confirmPage(lang, account.email, region));
});

/** 最終確認の画面から、支払いの画面へ送る。買い終えたらアカウントの画面へ戻す。 */
accountApp.post('/buy', async (c) => {
	const lang = resolveLang(c);
	const form = await c.req.parseBody();
	const next = safeNext(formString(form, 'next'));
	const account = await currentAccount(c);
	if (!account) return c.html(signIn(c, lang, next), 401);
	const config = stripeConfig(c.env);
	if (!config)
		return c.html(messagePage(lang, messages[lang].buyTitle, messages[lang].notForSale), 404);
	// 支払いの画面の予約を取る。開いている画面があれば同じ画面へ送り、2つのタブや、確かめを待つ間の買い直しで
	// 二重に払わせないように。払い終えた画面を開き直すと、Stripe が払い終えたことを示す。
	const t = now();
	await c.env.DB.batch([
		c.env.DB.prepare('DELETE FROM checkouts WHERE expires_at <= ?').bind(t),
		c.env.DB.prepare(
			`INSERT INTO checkouts (id, account_id, next, lang, expires_at, managed_payments, buyer_country)
			 VALUES (?, ?, ?, ?, ?, ?, ?)
			 ON CONFLICT (account_id) DO NOTHING`
		).bind(
			randomHex(16),
			account.id,
			next,
			lang,
			t + CHECKOUT_TTL,
			usesManagedPayments(buyerCountry(c)) ? 1 : 0,
			buyerCountry(c) ?? null
		)
	]);
	const checkout = (await c.env.DB.prepare(
		`SELECT id, next, lang, url, expires_at, managed_payments, buyer_country
		 FROM checkouts WHERE account_id = ?`
	)
		.bind(account.id)
		.first<{
			id: string;
			next: string;
			lang: Lang;
			url: string | null;
			expires_at: number;
			managed_payments: number;
			buyer_country: string | null;
		}>())!;
	const managedPayments = checkout.managed_payments === 1;
	if (checkout.url) return c.redirect(checkout.url, 303);
	// 画面がまだ無い予約 (ほかのタブが頼んでいる最中か、頼んだあとに失敗した) は、同じキーで頼み直す。
	// Stripe は作り終えた Session をそのまま返すので、画面は1つのまま。
	const origin = new URL(c.req.url).origin;
	const done = new URLSearchParams({ next: checkout.next, lang: checkout.lang });
	let session: { url: string };
	try {
		session = await createCheckoutSession(config, {
			accountId: account.id,
			email: account.email,
			lang: checkout.lang,
			// `{CHECKOUT_SESSION_ID}` は Stripe が置き換える (URLSearchParams に通すと括弧が符号化されるので、後ろに足す)。
			successUrl: `${origin}${ACCOUNT}/buy/done?${done}&session_id={CHECKOUT_SESSION_ID}`,
			// Stripe の画面で戻ったら、買い直せる最終確認の画面へ戻す。
			cancelUrl: `${origin}${ACCOUNT}/buy?lang=${checkout.lang}`,
			expiresAt: checkout.expires_at,
			submitMessage: managedPayments
				? undefined
				: messages[checkout.lang].checkoutNote(LEGAL_PAGES.tokushoho),
			managedPayments,
			buyerCountry: checkout.buyer_country ?? undefined,
			idempotencyKey: `weblav-checkout-${checkout.id}`
		});
	} catch (e) {
		// 同じ予約をほかのタブが頼んでいる最中。予約は残し、押し直してもらう。
		if (e instanceof StripeError && e.status === 409) {
			return c.html(messagePage(lang, messages[lang].buyTitle, messages[lang].buyBusy), 409);
		}
		// 応答が届かなかったときは、Stripe が作ったかどうか分からない。予約を残し、押し直したら同じキーで頼み直す。
		if (!(e instanceof StripeError)) throw e;
		// Stripe が断ったときは予約を外し、押し直したら新しく頼めるように (同じキーでは、Stripe は同じ失敗を返し続ける)。
		await c.env.DB.prepare('DELETE FROM checkouts WHERE id = ? AND url IS NULL')
			.bind(checkout.id)
			.run();
		throw e;
	}
	await c.env.DB.prepare('UPDATE checkouts SET url = ? WHERE id = ?')
		.bind(session.url, checkout.id)
		.run();
	return c.redirect(session.url, 303);
});

/**
 * 支払いから戻った先。webhook が届いて残高が付くのを待ち、付いたら予約を外して `next` へ進む。
 * 予約を外すのは、次の買い足しで、払い終えた支払いの画面へ送らないため。
 */
accountApp.get('/buy/done', async (c) => {
	const lang = resolveLang(c);
	const next = safeNext(c.req.query('next'));
	const account = await currentAccount(c);
	if (!account) return c.html(signIn(c, lang, next));
	const bought = await c.env.DB.prepare(
		`SELECT 1 FROM grants JOIN purchases ON purchases.id = grants.purchase_id
		 WHERE purchases.stripe_checkout_session_id = ? AND grants.account_id = ?`
	)
		.bind(c.req.query('session_id') ?? '', account.id)
		.first();
	if (bought) {
		await c.env.DB.prepare('DELETE FROM checkouts WHERE account_id = ?').bind(account.id).run();
		const url = new URL(next, c.req.url);
		url.searchParams.set('bought', '1');
		return c.redirect(`${url.pathname}${url.search}`, 303);
	}
	const tries = Number(c.req.query('tries') ?? '0') || 0;
	const retry = new URL(c.req.url);
	retry.searchParams.set('tries', String(tries + 1));
	return c.html(
		checkingPurchasePage(lang, `${retry.pathname}${retry.search}`, tries < PURCHASE_CHECKS)
	);
});

// 料金のページ。売っていない間は、買うボタンを外して返す (→ src/sale.ts)。
// 末尾の `/` は、`strict: false` が照らす前に落とすので、経路には付けない。
app.get('/pricing', async (c) => {
	if (stripeConfig(c.env)) return c.env.ASSETS.fetch(c.req.raw);
	// 条件付きの取得にしない。304 が返ると、書き換える本文が無い。
	return closeSale(await c.env.ASSETS.fetch(new Request(c.req.url)));
});

app.route(ACCOUNT, accountApp);

export default app;
