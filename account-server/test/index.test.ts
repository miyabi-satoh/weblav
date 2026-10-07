import { env, exports } from 'cloudflare:workers';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	checkAuth,
	deriveSecret,
	formatRelease,
	formatRequest,
	installationId,
	linkKey,
	relinkTag,
	type Plan
} from '../src/link';
import { DELETE_ACCOUNT_STATEMENTS } from '../src/account-deletion';
import { base64urlBytes, now, toHex, utf8 } from '../src/util';

const ORIGIN = 'http://account.test';

/**
 * 送り主 (IP) は、指定が無ければ毎回変える。IP ごとの上限を確かめるテスト以外で当たらないように。
 * 言語は、指定が無ければ日本語のブラウザにする。
 */
function request(
	path: string,
	init: RequestInit & { cookie?: string; ip?: string; country?: string } = {}
) {
	const headers = new Headers(init.headers);
	if (!headers.has('accept-language')) headers.set('accept-language', 'ja');
	if (init.cookie) headers.set('cookie', init.cookie);
	headers.set('cf-connecting-ip', init.ip ?? crypto.randomUUID());
	// アクセス元の国 (Cloudflare が付ける)。指定が無ければ日本にする。
	const cf = { country: init.country ?? 'JP' };
	return exports.default.fetch(new Request(`${ORIGIN}${path}`, { ...init, headers, cf }));
}

/** 画面のフォームから送ったのと同じ形 (Origin 付き) で送る。 */
function postForm(
	path: string,
	fields: Record<string, string>,
	cookie?: string,
	{ origin = ORIGIN, ip, country }: { origin?: string; ip?: string; country?: string } = {}
) {
	return request(path, {
		method: 'POST',
		headers: { origin, 'content-type': 'application/x-www-form-urlencoded' },
		body: new URLSearchParams(fields).toString(),
		cookie,
		ip,
		country,
		redirect: 'manual'
	});
}

/** メールのリンクでサインインし、セッションの Cookie を返す。送ったメールはログから拾う。 */
async function signIn(email: string, next = '/account/') {
	const log = vi.spyOn(console, 'log').mockImplementation(() => {});
	const sent = await postForm('/account/login/email', { email, next });
	expect(sent.status).toBe(200);
	const mail = String(log.mock.calls.at(-1)?.[0]);
	log.mockRestore();
	const token = new URL(mail.match(/http\S+/)![0]).searchParams.get('token')!;
	const verified = await postForm('/account/login/email/verify', { token });
	expect(verified.status).toBe(303);
	return {
		cookie: verified.headers.get('set-cookie')!.split(';')[0],
		location: verified.headers.get('location'),
		token
	};
}

/** アカウントに Pro (サブスクの行) を付ける。払い終えた期間の終わりは、指定が無ければ30日後。 */
async function grantPro(email: string, plan: Plan, paidThrough = now() + 30 * 86_400) {
	await env.DB.prepare(
		`INSERT INTO subscriptions (id, account_id, plan, paid_through, status, created_at)
		 SELECT ?, id, ?, ?, 'active', 0 FROM accounts WHERE email = ?`
	)
		.bind(`test-${crypto.randomUUID()}`, plan, paidThrough, email)
		.run();
}

/**
 * WebLAV と同じ形の申し込みを作る。秘密は窓口の関数で導く (WebLAV と同じ値になることは link.test.ts が確かめる)。
 * `previous` は、結び直すときの前の秘密。
 */
async function makeRequest({
	createdAt = now(),
	previous
}: { createdAt?: number; previous?: Uint8Array } = {}) {
	const pair = (await crypto.subtle.generateKey({ name: 'X25519' }, true, [
		'deriveBits'
	])) as CryptoKeyPair;
	const publicKey = new Uint8Array(
		(await crypto.subtle.exportKey('raw', pair.publicKey)) as ArrayBuffer
	);
	const key = await linkKey(env);
	const secret = (await deriveSecret(key, publicKey))!;
	const relink = previous
		? {
				installation: await installationId(previous),
				tag: await relinkTag(previous, publicKey)
			}
		: undefined;
	return {
		raw: formatRequest({ kid: key.kid, createdAt, publicKey, relink }),
		secret,
		id: await installationId(secret)
	};
}

/** WebLAV のサーバーと同じ形で確かめる。 */
async function check(
	secret: Uint8Array,
	{ name = '教室', pc = '', ip }: { name?: string; pc?: string; ip?: string } = {}
) {
	return request('/v1/installations/check', {
		method: 'POST',
		headers: { 'content-type': 'application/json' },
		body: JSON.stringify({
			installation: await installationId(secret),
			auth: toHex(await checkAuth(secret)),
			name,
			pc
		}),
		ip
	});
}

/** 結ぶ画面のボタンを押す。 */
function link(cookie: string, raw: string, name = '教室', pc = '') {
	return postForm('/account/link', { r: raw, name, pc }, cookie);
}

/** 結んだ WebLAV の数。 */
async function installationsOf(email: string) {
	const { results } = await env.DB.prepare(
		`SELECT installations.* FROM installations JOIN accounts ON accounts.id = installations.account_id
		 WHERE accounts.email = ? ORDER BY created_at, id`
	)
		.bind(email)
		.all<{ id: string; name: string; pc_name: string; removed_at: number | null }>();
	return results;
}

/** WebLAV の `src/pro.rs` と同じ手順で確かめる。 */
async function verifyProof(proof: string) {
	const [payload, signature] = proof.split('.');
	const jwk = JSON.parse(env.PROOF_SIGNING_KEY);
	const key = await crypto.subtle.importKey(
		'jwk',
		{ kty: jwk.kty, crv: jwk.crv, x: jwk.x },
		{ name: 'Ed25519' },
		false,
		['verify']
	);
	const ok = await crypto.subtle.verify(
		'Ed25519',
		key,
		base64urlBytes(signature),
		base64urlBytes(payload)
	);
	return { ok, claims: JSON.parse(new TextDecoder().decode(base64urlBytes(payload))) };
}

/** 応答で始まったセッションの Cookie。無ければ `undefined`。 */
function sessionCookie(res: Response) {
	return res.headers
		.getSetCookie()
		.find((c) => c.startsWith('session='))
		?.split(';')[0];
}

afterEach(() => vi.restoreAllMocks());

/** 全部の入口にかかる守り (Origin の確かめ・サインイン)。入口の一覧を表にして回す。 */
describe('guards', () => {
	/** 人が開く画面のうち、書き込む (POST の) 入口。どれもフォームから送る。 */
	const FORM_POSTS = [
		'/account/login/email',
		'/account/login/email/verify',
		'/account/logout',
		'/account/link',
		'/account/release',
		'/account/installations/remove',
		'/account/billing',
		'/account/buy',
		'/account/transfer'
	];

	/** サインインが要る入口。POST は 401、GET はサインインの画面をそのまま出す (戻り先を持って入り直せるように)。 */
	const SIGN_IN_REQUIRED: [method: 'GET' | 'POST', path: string, status: number][] = [
		['GET', '/account/', 200],
		['GET', '/account/link?r=R&name=x', 200],
		['GET', '/account/buy?plan=year', 200],
		['GET', '/account/buy/done', 200],
		['GET', '/account/transfer', 200],
		['POST', '/account/link', 401],
		['POST', '/account/installations/remove', 401],
		['POST', '/account/billing', 401],
		['POST', '/account/buy', 401],
		['POST', '/account/transfer', 401]
	];

	// 移す画面は Pro が無いときも 403 を返すので、Origin の確かめで断ったこと (hono/csrf の本文) まで見る。
	it('rejects form posts from other sites on every entry', async () => {
		const email = 'csrf@example.com';
		const { cookie } = await signIn(email);
		await grantPro(email, 'personal');
		for (const path of FORM_POSTS) {
			// Apple のサイトから送ってよいのは、Apple から戻る先 (/account/login/apple/callback) だけ。
			for (const origin of ['https://evil.test', 'https://appleid.apple.com']) {
				const res = await postForm(path, { next: '/account/' }, cookie, { origin });
				expect([res.status, await res.text()], `POST ${path} (${origin})`).toEqual([
					403,
					'Forbidden'
				]);
			}
		}
		// 断った要求は何も変えない (サインアウトしていない)。
		expect(await (await request('/account/', { cookie })).text()).toContain(email);
	});

	it('asks to sign in on every entry that needs an account', async () => {
		for (const [method, path, status] of SIGN_IN_REQUIRED) {
			const res =
				method === 'GET' ? await request(path) : await postForm(path, { next: '/account/' });
			expect(res.status, `${method} ${path}`).toBe(status);
			expect(await res.text(), `${method} ${path}`).toContain('action="/account/login/email"');
		}
	});
});

describe('linking', () => {
	function linkPath(raw: string, name = '教室') {
		return `/account/link?${new URLSearchParams({ r: raw, name })}`;
	}

	it('asks to sign in, offers Pro, then links and shows the code', async () => {
		const email = 'linker@example.com';
		const req = await makeRequest();
		const before = await request(linkPath(req.raw));
		expect(await before.text()).toContain('action="/account/login/email"');
		const { cookie, location } = await signIn(email, linkPath(req.raw));
		expect(location).toBe(linkPath(req.raw));

		// Pro が無ければ料金ページへ案内し、申し込んだ後にこの画面へ戻れるよう戻り先を渡す。
		const noPro = await (await request(linkPath(req.raw), { cookie })).text();
		expect(noPro).toContain(`href="/pricing/?${new URLSearchParams({ next: linkPath(req.raw) })}"`);
		expect((await link(cookie, req.raw)).status).toBe(200);
		expect(await installationsOf(email)).toEqual([]);

		await grantPro(email, 'personal');
		const confirm = await (await request(linkPath(req.raw), { cookie })).text();
		expect(confirm).toContain('「教室」をこのアカウントに登録します');
		expect(confirm).toContain('0 / 3 台');
		expect(confirm).toContain('自分の WebLAV の画面から開いたのでなければ');

		const log = vi.spyOn(console, 'log').mockImplementation(() => {});
		const linked = await link(cookie, req.raw);
		expect(linked.status).toBe(200);
		const page = await linked.text();
		const code = page.match(/[0-9A-Z]{5}-[0-9A-Z]{5}-[0-9A-Z]{5}/)![0];
		// 返しのコードをメールでも送る。
		expect(log.mock.calls.some((c) => String(c[0]).includes(code))).toBe(true);
		log.mockRestore();
		expect(await installationsOf(email)).toMatchObject([{ id: req.id, name: '教室' }]);

		// ネットにつながる WebLAV は、確かめで署名付きの証明を受け取る。
		const res = await check(req.secret);
		const body = await res.json<{ status: string; proof: string; email: string }>();
		expect(body.status).toBe('ok');
		expect(body.email).toBe(email);
		const { ok, claims } = await verifyProof(body.proof);
		expect(ok).toBe(true);
		expect(claims).toMatchObject({
			v: 2,
			proof_kid: 'dev-local',
			installation: req.id,
			plan: 'personal'
		});
		// 期限は、払い終えた期間の終わり (30日後) にカードの猶予 (7日) を足した日。
		expect(claims.expires_at - now()).toBeGreaterThan(36 * 86_400);
	});

	it('caps the proof at 45 days even for a yearly plan', async () => {
		const email = 'yearly@example.com';
		const { cookie } = await signIn(email);
		const at = now();
		await grantPro(email, 'personal', at + 365 * 86_400);
		const req = await makeRequest();
		await link(cookie, req.raw);
		const body = await (await check(req.secret)).json<{ proof: string }>();
		const { claims } = await verifyProof(body.proof);
		expect(claims.expires_at).toBeLessThanOrEqual(at + 46 * 86_400);
		expect(claims.expires_at).toBeGreaterThan(at + 44 * 86_400);
	});

	it('shows the same code again for the same request, and refuses it from another account', async () => {
		const req = await makeRequest();
		const one = await signIn('same1@example.com');
		await grantPro('same1@example.com', 'personal');
		const first = await (await link(one.cookie, req.raw)).text();
		const again = await (await request(linkPath(req.raw), { cookie: one.cookie })).text();
		const code = (s: string) => s.match(/[0-9A-Z]{5}-[0-9A-Z]{5}-[0-9A-Z]{5}/)![0];
		expect(code(again)).toBe(code(first));
		expect(await installationsOf('same1@example.com')).toHaveLength(1);
		const two = await signIn('same2@example.com');
		await grantPro('same2@example.com', 'personal');
		expect((await link(two.cookie, req.raw)).status).toBe(409);
	});

	it('refuses malformed, old and released requests', async () => {
		const { cookie } = await signIn('bad-request@example.com');
		await grantPro('bad-request@example.com', 'personal');
		expect((await link(cookie, 'NOT-A-REQUEST')).status).toBe(400);
		const old = await makeRequest({ createdAt: now() - 25 * 3600 });
		expect((await link(cookie, old.raw)).status).toBe(400);
		const req = await makeRequest();
		await link(cookie, req.raw);
		const released = await request('/v1/installations/release', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ code: await formatRelease(req.secret) })
		});
		expect(released.status).toBe(204);
		expect(await installationsOf('bad-request@example.com')).toEqual([]);
		// 外した後に古い QR コードを読み直しても、WebLAV が捨てた秘密の枠は作らない。
		expect((await link(cookie, req.raw)).status).toBe(400);
	});

	it('stops at the limit, and frees a slot only when the WebLAV receives the removal', async () => {
		const email = 'limit@example.com';
		const { cookie } = await signIn(email);
		await grantPro(email, 'personal');
		const reqs = [await makeRequest(), await makeRequest(), await makeRequest()];
		for (const r of reqs) expect((await link(cookie, r.raw)).status).toBe(200);
		const fourth = await makeRequest();
		const atLimit = await (await link(cookie, fourth.raw)).text();
		expect(atLimit).toContain('登録できる台数の上限に達しています');
		expect(atLimit).toContain('action="/account/installations/remove"');

		// 窓口で外しても、出した許可の期限までは台数に数える。
		await postForm('/account/installations/remove', { id: reqs[0].id, next: '/account/' }, cookie);
		expect(await (await link(cookie, fourth.raw)).text()).toContain('登録できる台数の上限');
		const home = await (await request('/account/', { cookie })).text();
		expect(home).toContain('その PC が受け取るまで');
		// WebLAV が確かめで外されたことを受け取ると、枠が空く。
		expect(await (await check(reqs[0].secret)).json()).toEqual({ status: 'unbound' });
		expect((await link(cookie, fourth.raw)).status).toBe(200);
		expect(await installationsOf(email)).toHaveLength(3);
	});

	it('removes at once a WebLAV that never got a permission', async () => {
		const email = 'never@example.com';
		const { cookie } = await signIn(email);
		await grantPro(email, 'personal');
		const req = await makeRequest();
		await link(cookie, req.raw);
		await env.DB.prepare('UPDATE installations SET permission_expires_at = NULL WHERE id = ?')
			.bind(req.id)
			.run();
		await postForm('/account/installations/remove', { id: req.id, next: '/account/' }, cookie);
		expect(await installationsOf(email)).toEqual([]);
	});

	it('relinks the same WebLAV without using another slot', async () => {
		const email = 'relink@example.com';
		const { cookie } = await signIn(email);
		await grantPro(email, 'personal');
		const first = await makeRequest();
		await link(cookie, first.raw);
		const second = await makeRequest({ previous: first.secret });
		expect(await (await request(linkPath(second.raw), { cookie })).text()).toContain(
			'登録し直します'
		);
		expect((await link(cookie, second.raw)).status).toBe(200);
		expect(await installationsOf(email)).toMatchObject([{ id: second.id }]);
		// 前の秘密ではもう確かめられない。
		expect(await (await check(first.secret)).json()).toEqual({ status: 'unbound' });
		expect((await (await check(second.secret)).json<{ status: string }>()).status).toBe('ok');
	});

	it('answers no_plan without Pro, and over_limit for the newest beyond the limit', async () => {
		const email = 'downgrade@example.com';
		const { cookie } = await signIn(email);
		await grantPro(email, 'organization');
		const reqs = [];
		for (let i = 0; i < 4; i++) {
			const r = await makeRequest();
			await link(cookie, r.raw);
			reqs.push(r);
		}
		// 組織向けをやめて個人向け (3台) に下げる。
		await env.DB.prepare(
			`UPDATE subscriptions SET revoked_at = 1
			 WHERE account_id = (SELECT id FROM accounts WHERE email = ?)`
		)
			.bind(email)
			.run();
		expect(await (await check(reqs[0].secret)).json()).toEqual({ status: 'no_plan', email });
		await grantPro(email, 'personal');
		for (const r of reqs.slice(0, 3)) {
			expect((await (await check(r.secret)).json<{ status: string }>()).status).toBe('ok');
		}
		expect(await (await check(reqs[3].secret)).json()).toEqual({ status: 'over_limit', email });
		expect(await (await request('/account/', { cookie })).text()).toContain(
			'上限 (3 台) を超えています'
		);
	});

	it('prefers the organization plan when the account has both', async () => {
		const email = 'both@example.com';
		const { cookie } = await signIn(email);
		await grantPro(email, 'personal');
		await grantPro(email, 'organization');
		const req = await makeRequest();
		await link(cookie, req.raw);
		const body = await (await check(req.secret)).json<{ proof: string }>();
		expect((await verifyProof(body.proof)).claims.plan).toBe('organization');
	});

	it('shows the PC name beside the site name, and keeps it when an empty one comes', async () => {
		const email = 'pc-names@example.com';
		const { cookie } = await signIn(email);
		await grantPro(email, 'personal');
		const req = await makeRequest();
		await link(cookie, req.raw, 'WebLAV', 'home-pc');
		expect((await installationsOf(email))[0].pc_name).toBe('home-pc');
		await check(req.secret, { pc: '' });
		expect((await installationsOf(email))[0].pc_name).toBe('home-pc');
		await check(req.secret, { pc: 'office-pc' });
		expect((await installationsOf(email))[0].pc_name).toBe('office-pc');
		const home = await (await request('/account/', { cookie })).text();
		expect(home).toContain('教室 (office-pc)');
	});

	it('keeps the name when an empty one comes, and answers unbound to strangers', async () => {
		const email = 'names@example.com';
		const { cookie } = await signIn(email);
		await grantPro(email, 'personal');
		const req = await makeRequest();
		await link(cookie, req.raw);
		await check(req.secret, { name: '' });
		expect((await installationsOf(email))[0].name).toBe('教室');
		await check(req.secret, { name: '本校' });
		expect((await installationsOf(email))[0].name).toBe('本校');
		const stranger = await makeRequest();
		expect(await (await check(stranger.secret)).json()).toEqual({ status: 'unbound' });
		const wrongAuth = await request('/v1/installations/check', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ installation: req.id, auth: '00', name: '' })
		});
		expect(await wrongAuth.json()).toEqual({ status: 'unbound' });
	});

	it('limits checks per sender', async () => {
		const req = await makeRequest();
		const ip = crypto.randomUUID();
		for (let i = 0; i < 10; i++) await check(req.secret, { ip });
		expect((await check(req.secret, { ip })).status).toBe(429);
	});

	it('takes the release from the QR code page only by the button', async () => {
		const email = 'release-page@example.com';
		const { cookie } = await signIn(email);
		await grantPro(email, 'personal');
		const req = await makeRequest();
		await link(cookie, req.raw);
		const code = await formatRelease(req.secret);
		const page = await (await request(`/account/release?c=${code}`)).text();
		expect(page).toContain('action="/account/release"');
		expect(await installationsOf(email)).toHaveLength(1);
		const done = await postForm('/account/release', { c: code });
		expect(await done.text()).toContain('登録を解除しました');
		expect(await installationsOf(email)).toEqual([]);
		expect((await postForm('/account/release', { c: 'XYZ' })).status).toBe(400);
	});
});

describe('email sign-in', () => {
	it('does not sign in by merely opening the link, and each link works once', async () => {
		const { token } = await signIn('once@example.com');
		const opened = await request(`/account/login/email?token=${token}`);
		expect(opened.headers.get('set-cookie')).toBeNull();
		const reused = await postForm('/account/login/email/verify', { token });
		expect(reused.status).toBe(400);
	});

	it('rejects an expired link', async () => {
		const log = vi.spyOn(console, 'log').mockImplementation(() => {});
		await postForm('/account/login/email', { email: 'late@example.com', next: '/account/' });
		const mail = String(log.mock.calls.at(-1)?.[0]);
		const token = new URL(mail.match(/http\S+/)![0]).searchParams.get('token')!;
		await env.DB.prepare(
			"UPDATE email_logins SET expires_at = 0 WHERE email = 'late@example.com'"
		).run();
		expect((await postForm('/account/login/email/verify', { token })).status).toBe(400);
	});

	it('forgets expired sessions', async () => {
		const { cookie } = await signIn('stale@example.com');
		await env.DB.prepare('UPDATE sessions SET expires_at = 0').run();
		expect(await (await request('/account/', { cookie })).text()).toContain('WebLAV にサインイン');
	});

	it('limits links per sender across addresses', async () => {
		vi.spyOn(console, 'log').mockImplementation(() => {});
		const ip = '198.51.100.2';
		const send = (email: string) =>
			postForm('/account/login/email', { email, next: '/account/' }, undefined, { ip });
		for (let i = 0; i < 5; i++) expect((await send(`a${i}@example.com`)).status).toBe(200);
		expect((await send('a9@example.com')).status).toBe(429);
	});

	it('keeps the same account for the same address in any case', async () => {
		await signIn('Same@Example.com');
		await signIn('same@example.com');
		const { n } = (await env.DB.prepare(
			"SELECT count(*) AS n FROM accounts WHERE email = 'same@example.com'"
		).first<{ n: number }>())!;
		expect(n).toBe(1);
	});

	it('limits how many links go to one address per hour', async () => {
		vi.spyOn(console, 'log').mockImplementation(() => {});
		// 送り主の IP を毎回変え、IP ごとの上限ではなくアドレスごとの上限で止まるのを見る。同時に送っても超えない。
		const email = 'flood@example.com';
		const statuses = await Promise.all(
			Array.from({ length: 8 }, (_, i) =>
				postForm('/account/login/email', { email, next: '/account/' }, undefined, {
					ip: `203.0.113.${i}`
				})
			)
		).then((res) => res.map((r) => r.status));
		expect(statuses.filter((s) => s === 200)).toHaveLength(5);
		expect(statuses.filter((s) => s === 429)).toHaveLength(3);
	});

	it('says that signing in agrees to the terms and the privacy policy', async () => {
		const page = await (await request('/account/?lang=ja')).text();
		const consent = page.slice(page.indexOf('サインインすると'), page.indexOf('に同意'));
		expect(consent).toContain('href="/terms/"');
		expect(consent).toContain('href="/privacy/"');
		expect(page).toContain('アメリカ合衆国の事業者');
	});

	it('signs out', async () => {
		const { cookie } = await signIn('bye@example.com');
		expect(await (await request('/account/', { cookie })).text()).toContain('bye@example.com');
		await postForm('/account/logout', { next: '/account/' }, cookie);
		expect(await (await request('/account/', { cookie })).text()).toContain('WebLAV にサインイン');
	});
});

describe('Google sign-in', () => {
	/** Google の画面へ送り、戻ってきたときに要る Cookie と state・nonce を返す。 */
	async function startGoogle(next = '/account/') {
		const res = await request(`/account/login/google?next=${encodeURIComponent(next)}`, {
			redirect: 'manual'
		});
		expect(res.status).toBe(303);
		const to = new URL(res.headers.get('location')!);
		expect(`${to.origin}${to.pathname}`).toBe('https://accounts.google.com/o/oauth2/v2/auth');
		expect(to.searchParams.get('code_challenge_method')).toBe('S256');
		expect(to.searchParams.get('redirect_uri')).toBe(`${ORIGIN}/account/login/google/callback`);
		return {
			cookie: res.headers.get('set-cookie')!.split(';')[0],
			state: to.searchParams.get('state')!,
			nonce: to.searchParams.get('nonce')!
		};
	}

	/** Google のトークンのエンドポイントの応答を差し替え、ID トークンの中身を `claims` にする。 */
	function googleAnswers(claims: Record<string, unknown>) {
		const body = btoa(JSON.stringify(claims)).replaceAll('=', '');
		return vi
			.spyOn(globalThis, 'fetch')
			.mockImplementation(async () => Response.json({ id_token: `e30.${body}.sig` }));
	}

	async function back(
		flow: { cookie: string; state: string; nonce: string },
		claims: Record<string, unknown> = {},
		state = flow.state
	) {
		googleAnswers({
			iss: 'https://accounts.google.com',
			aud: 'google-client',
			exp: now() + 300,
			nonce: flow.nonce,
			sub: 'google-sub-1',
			email: 'g@example.com',
			email_verified: true,
			// Google Workspace のアカウント。Google が持ち主を確かだと言える。
			hd: 'example.com',
			...claims
		});
		return request(`/account/login/google/callback?code=c&state=${state}`, {
			cookie: flow.cookie,
			redirect: 'manual'
		});
	}

	it('offers Google on the sign-in page', async () => {
		const page = await (await request('/account/?lang=ja')).text();
		expect(page).toContain('href="/account/login/google?next=%2Faccount%2F"');
	});

	it('signs in a new account and comes back to the same account next time', async () => {
		const first = await back(await startGoogle('/account/transfer'), {
			sub: 'g-new',
			email: 'G-New@Example.com'
		});
		expect(first.status).toBe(303);
		expect(first.headers.get('location')).toBe('/account/transfer');
		const home = await (await request('/account/', { cookie: sessionCookie(first) })).text();
		expect(home).toContain('g-new@example.com');
		// Google でメールアドレスを変えても、識別子で同じアカウントに入る。
		const again = await back(await startGoogle(), { sub: 'g-new', email: 'renamed@example.com' });
		const page = await (await request('/account/', { cookie: sessionCookie(again) })).text();
		expect(page).toContain('g-new@example.com');
	});

	it('joins the account that signed in by email with the same verified address', async () => {
		await signIn('email-then-google@example.com');
		await grantPro('email-then-google@example.com', 'personal');
		const res = await back(await startGoogle(), {
			sub: 'g-both',
			email: 'email-then-google@example.com'
		});
		const home = await (await request('/account/', { cookie: sessionCookie(res) })).text();
		expect(home).toContain('Pro (個人向け)');
	});

	it('refuses addresses whose owner Google cannot vouch for, and keeps the way back', async () => {
		for (const claims of [
			{ sub: 'g-unverified', email: 'unverified@example.com', email_verified: false },
			// Gmail でも Workspace でもないアドレスは、作ったときに確かめただけで、今の持ち主かは分からない。
			{ sub: 'g-third-party', email: 'third-party@example.com', hd: undefined }
		]) {
			vi.restoreAllMocks();
			const res = await back(await startGoogle('/account/transfer'), claims);
			expect(res.status).toBe(400);
			expect(sessionCookie(res)).toBeUndefined();
			expect(await res.text()).toContain('value="/account/transfer"');
			expect(
				await env.DB.prepare('SELECT 1 FROM accounts WHERE email = ?').bind(claims.email).first()
			).toBeNull();
		}
	});

	it('accepts a Gmail address without a hosted domain', async () => {
		const res = await back(await startGoogle(), {
			sub: 'g-gmail',
			email: 'someone@gmail.com',
			hd: undefined
		});
		expect(res.status).toBe(303);
		expect(sessionCookie(res)).toBeDefined();
	});

	it('does not link a second Google account to the same account', async () => {
		await back(await startGoogle(), { sub: 'g-first', email: 'twice-g@example.com' });
		const res = await back(await startGoogle(), { sub: 'g-second', email: 'twice-g@example.com' });
		expect(res.status).toBe(409);
		expect(sessionCookie(res)).toBeUndefined();
	});

	it('rejects a wrong state, nonce, audience or an expired token', async () => {
		for (const [claims, state] of [
			[{}, 'wrong-state'],
			[{ nonce: 'other' }, undefined],
			[{ aud: 'someone-else' }, undefined],
			[{ iss: 'https://evil.test' }, undefined],
			[{ exp: now() - 1 }, undefined]
		] as const) {
			vi.restoreAllMocks();
			const flow = await startGoogle();
			const res = await back(
				flow,
				{ sub: 'g-bad', email: 'bad@example.com', ...claims },
				state ?? flow.state
			);
			expect(res.status).toBe(400);
			expect(sessionCookie(res)).toBeUndefined();
		}
		// 往復を始めていない (Cookie が無い) ときも。
		const res = await request('/account/login/google/callback?code=c&state=s', {
			redirect: 'manual'
		});
		expect(res.status).toBe(400);
	});

	it('sends the code with the verifier and the client secret', async () => {
		const flow = await startGoogle();
		const res = await back(flow, { sub: 'g-pkce', email: 'pkce@example.com' });
		expect(res.status).toBe(303);
		const [url, init] = vi.mocked(globalThis.fetch).mock.calls[0];
		expect(url).toBe('https://oauth2.googleapis.com/token');
		const sent = new URLSearchParams(String(init!.body));
		expect(sent.get('client_secret')).toBe('google-secret');
		expect(sent.get('grant_type')).toBe('authorization_code');
		expect(sent.get('code_verifier')).toMatch(/^[0-9a-f]{64}$/);
	});
});

describe('Apple sign-in', () => {
	const APPLE = 'https://appleid.apple.com';

	/** Apple の画面へ送り、戻ってきたときに要る Cookie と state・nonce を返す。 */
	async function startApple(next = '/account/', lang = 'ja') {
		const res = await request(`/account/login/apple?next=${encodeURIComponent(next)}`, {
			headers: { 'accept-language': lang },
			redirect: 'manual'
		});
		expect(res.status).toBe(303);
		const to = new URL(res.headers.get('location')!);
		expect(`${to.origin}${to.pathname}`).toBe(`${APPLE}/auth/authorize`);
		expect(to.searchParams.get('client_id')).toBe('com.example.web');
		expect(to.searchParams.get('response_mode')).toBe('form_post');
		expect(to.searchParams.get('scope')).toBe('email');
		expect(to.searchParams.get('redirect_uri')).toBe(`${ORIGIN}/account/login/apple/callback`);
		return {
			cookie: res.headers.get('set-cookie')!.split(';')[0],
			state: to.searchParams.get('state')!,
			nonce: to.searchParams.get('nonce')!
		};
	}

	/** Apple のサイトから戻り先へ POST する。トークンのエンドポイントは、claims の ID トークンを返す。 */
	async function back(
		flow: { cookie: string; state: string; nonce: string },
		claims: Record<string, unknown> = {},
		fields: Record<string, string> = { code: 'c', state: flow.state },
		origin = APPLE
	) {
		const body = btoa(
			JSON.stringify({
				iss: APPLE,
				aud: 'com.example.web',
				exp: now() + 300,
				nonce: flow.nonce,
				sub: 'apple-sub-1',
				email: 'a@example.com',
				email_verified: 'true',
				...claims
			})
		).replaceAll('=', '');
		const token = vi
			.spyOn(globalThis, 'fetch')
			.mockImplementation(async () => Response.json({ id_token: `e30.${body}.sig` }));
		const res = await postForm('/account/login/apple/callback', fields, flow.cookie, { origin });
		return { res, token };
	}

	it('offers Apple on the sign-in page', async () => {
		const page = await (await request('/account/?lang=ja')).text();
		expect(page).toContain('href="/account/login/apple?next=%2Faccount%2F"');
		expect(page).toContain('Appleでサインイン');
	});

	it('signs in with a client secret signed by the Apple key and comes back to the same account', async () => {
		const { res, token } = await back(await startApple('/account/transfer'), {
			sub: 'a-new',
			email: 'A-New@Example.com'
		});
		expect(res.status).toBe(303);
		expect(res.headers.get('location')).toBe('/account/transfer');
		const home = await (await request('/account/', { cookie: sessionCookie(res) })).text();
		expect(home).toContain('a-new@example.com');

		const [url, init] = token.mock.calls[0];
		expect(url).toBe(`${APPLE}/auth/token`);
		const sent = new URLSearchParams(String(init!.body));
		expect(sent.get('client_id')).toBe('com.example.web');
		expect(sent.get('grant_type')).toBe('authorization_code');
		expect(sent.get('redirect_uri')).toBe(`${ORIGIN}/account/login/apple/callback`);
		const [header, payload, signature] = sent.get('client_secret')!.split('.');
		const decode = (part: string) => JSON.parse(new TextDecoder().decode(base64urlBytes(part)));
		expect(decode(header)).toEqual({ alg: 'ES256', kid: 'KEY1234567' });
		const claims = decode(payload);
		expect(claims).toMatchObject({ iss: 'TEAM123456', aud: APPLE, sub: 'com.example.web' });
		expect(claims.exp - claims.iat).toBe(300);
		const key = await crypto.subtle.importKey(
			'jwk',
			JSON.parse(env.TEST_APPLE_PUBLIC_KEY),
			{ name: 'ECDSA', namedCurve: 'P-256' },
			false,
			['verify']
		);
		expect(
			await crypto.subtle.verify(
				{ name: 'ECDSA', hash: 'SHA-256' },
				key,
				base64urlBytes(signature),
				utf8(`${header}.${payload}`)
			)
		).toBe(true);

		vi.restoreAllMocks();
		// Apple でメールアドレスを変えても、識別子で同じアカウントに入る。
		const again = await back(await startApple(), { sub: 'a-new', email: 'renamed-a@example.com' });
		const page = await (await request('/account/', { cookie: sessionCookie(again.res) })).text();
		expect(page).toContain('a-new@example.com');
	});

	it('joins the account that signed in by email with the same verified address', async () => {
		await signIn('email-then-apple@example.com');
		await grantPro('email-then-apple@example.com', 'personal');
		const { res } = await back(await startApple(), {
			sub: 'a-both',
			email: 'email-then-apple@example.com',
			// Apple は真偽値で返すこともある。
			email_verified: true
		});
		const home = await (await request('/account/', { cookie: sessionCookie(res) })).text();
		expect(home).toContain('Pro (個人向け)');
	});

	it('does not link a second Apple account to the same account', async () => {
		await back(await startApple(), { sub: 'a-first', email: 'twice-a@example.com' });
		vi.restoreAllMocks();
		const { res } = await back(await startApple(), {
			sub: 'a-second',
			email: 'twice-a@example.com'
		});
		expect(res.status).toBe(409);
		expect(sessionCookie(res)).toBeUndefined();
	});

	it('goes back to the sign-in page quietly when cancelled on Apple', async () => {
		const flow = await startApple('/account/transfer');
		const { res, token } = await back(
			flow,
			{},
			{ error: 'user_cancelled_authorize', state: flow.state }
		);
		expect(res.status).toBe(200);
		expect(token).not.toHaveBeenCalled();
		const page = await res.text();
		expect(page).not.toContain('role="alert"');
		expect(page).toContain('value="/account/transfer"');
	});

	// ID トークンの確かめは finishAppleSignIn (Context を取る) の中にあり、切り出していないので、ここで場合ごとに確かめる。
	it('rejects a wrong state, nonce, audience, issuer, an expired token or an unverified email', async () => {
		for (const [claims, state] of [
			[{}, 'wrong-state'],
			[{ nonce: 'other' }, undefined],
			[{ aud: 'someone-else' }, undefined],
			[{ iss: 'https://evil.test' }, undefined],
			[{ exp: now() - 1 }, undefined],
			[{ email_verified: 'false' }, undefined],
			[{ email: undefined }, undefined]
		] as const) {
			vi.restoreAllMocks();
			const flow = await startApple('/account/transfer');
			const { res } = await back(
				flow,
				{ sub: 'a-bad', email: 'bad-a@example.com', ...claims },
				{ code: 'c', state: state ?? flow.state }
			);
			expect(res.status).toBe(400);
			expect(sessionCookie(res)).toBeUndefined();
			expect(await res.text()).toContain('value="/account/transfer"');
		}
		expect(
			await env.DB.prepare("SELECT 1 FROM accounts WHERE email = 'bad-a@example.com'").first()
		).toBeNull();
		// 往復を始めていない (Cookie が無い) ときも。
		const res = await postForm(
			'/account/login/apple/callback',
			{ code: 'c', state: 's' },
			undefined,
			{ origin: APPLE }
		);
		expect(res.status).toBe(400);
	});

	it('shows the page in the language used before going to Apple', async () => {
		// Apple からの POST には言語の Cookie が付かず、ブラウザの言語 (postForm は日本語) だけが届く。
		const flow = await startApple('/account/', 'en');
		const { res } = await back(flow, {}, { error: 'user_cancelled_authorize', state: flow.state });
		expect(await res.text()).toContain('Sign in to WebLAV');
	});

	it('accepts the way back even when Apple sends a null origin', async () => {
		const flow = await startApple();
		const { res } = await back(
			flow,
			{ sub: 'a-null-origin', email: 'null-origin@example.com' },
			{ code: 'c', state: flow.state },
			'null'
		);
		expect(res.status).toBe(303);
	});
});

describe('transfer', () => {
	async function plansOf(email: string) {
		const { results } = await env.DB.prepare(
			`SELECT plan FROM subscriptions JOIN accounts ON accounts.id = subscriptions.account_id
			 WHERE accounts.email = ? ORDER BY plan`
		)
			.bind(email)
			.all<{ plan: Plan }>();
		return results.map((r) => r.plan);
	}

	it('asks to confirm, then moves every Pro and tells both addresses', async () => {
		const { cookie } = await signIn('from@example.com');
		await grantPro('from@example.com', 'personal');
		await grantPro('from@example.com', 'organization');

		const home = await request('/account/', { cookie });
		expect(await home.text()).toContain('href="/account/transfer"');

		const confirm = await postForm('/account/transfer', { email: ' To@Example.com ' }, cookie);
		expect(confirm.status).toBe(200);
		expect(await confirm.text()).toContain('to@example.com へ移します');
		expect(await plansOf('from@example.com')).toEqual(['organization', 'personal']);

		const log = vi.spyOn(console, 'log').mockImplementation(() => {});
		const moved = await postForm(
			'/account/transfer',
			{ email: 'to@example.com', confirm: '1' },
			cookie
		);
		expect(moved.status).toBe(200);
		expect(await plansOf('from@example.com')).toEqual([]);
		expect(await plansOf('to@example.com')).toEqual(['organization', 'personal']);
		const mails = log.mock.calls.map((call) => String(call[0]));
		expect(mails.some((m) => m.startsWith('[mail] to=from@example.com'))).toBe(true);
		expect(
			mails.some((m) => m.startsWith('[mail] to=to@example.com') && m.includes('from@example.com'))
		).toBe(true);

		// 移し先でサインインすれば、その Pro で WebLAV を Pro にできる。
		const to = await signIn('to@example.com');
		expect(await (await request('/account/', { cookie: to.cookie })).text()).toContain('組織向け');
	});

	it('moves to an existing account and keeps what it had', async () => {
		const { cookie } = await signIn('from2@example.com');
		await signIn('to2@example.com');
		await grantPro('from2@example.com', 'personal');
		await grantPro('to2@example.com', 'personal');
		vi.spyOn(console, 'log').mockImplementation(() => {});
		const res = await postForm(
			'/account/transfer',
			{ email: 'to2@example.com', confirm: '1' },
			cookie
		);
		expect(res.status).toBe(200);
		expect(await plansOf('to2@example.com')).toEqual(['personal', 'personal']);
	});

	it('refuses without Pro, to itself, and to a malformed address', async () => {
		const { cookie } = await signIn('none@example.com');
		expect((await request('/account/transfer', { cookie })).status).toBe(403);
		expect(await (await request('/account/', { cookie })).text()).not.toContain(
			'href="/account/transfer"'
		);
		expect(
			(await postForm('/account/transfer', { email: 'x@example.com', confirm: '1' }, cookie)).status
		).toBe(403);

		await grantPro('none@example.com', 'personal');
		const self = await postForm(
			'/account/transfer',
			{ email: 'NONE@example.com', confirm: '1' },
			cookie
		);
		expect(self.status).toBe(400);
		// アドレスの形の場合分けは util.test.ts で確かめる。
		const bad = await postForm('/account/transfer', { email: 'nope', confirm: '1' }, cookie);
		expect(bad.status).toBe(400);
		expect(await plansOf('none@example.com')).toEqual(['personal']);
	});

	it('still moves when the notice cannot be sent, and says so', async () => {
		const { cookie } = await signIn('nomail@example.com');
		await grantPro('nomail@example.com', 'personal');
		vi.spyOn(console, 'error').mockImplementation(() => {});
		const log = vi.spyOn(console, 'log').mockImplementation((line: string) => {
			if (line.startsWith('[mail] to=nomail-to@')) throw new Error('send failed');
		});
		const res = await postForm(
			'/account/transfer',
			{ email: 'nomail-to@example.com', confirm: '1' },
			cookie
		);
		expect(res.status).toBe(200);
		expect(await res.text()).toContain('送れませんでした');
		// 送れた側の知らせは送り切る。
		expect(log.mock.calls.some((call) => String(call[0]).startsWith('[mail] to=nomail@'))).toBe(
			true
		);
		expect(await plansOf('nomail-to@example.com')).toEqual(['personal']);
	});

	it('limits moves per sender', async () => {
		const { cookie } = await signIn('many@example.com');
		vi.spyOn(console, 'log').mockImplementation(() => {});
		const statuses = [];
		for (let i = 0; i < 6; i++) {
			await grantPro('many@example.com', 'personal');
			const res = await postForm(
				'/account/transfer',
				{ email: `many-${i}@example.com`, confirm: '1' },
				cookie,
				{ ip: '192.0.2.40' }
			);
			statuses.push(res.status);
		}
		expect(statuses).toEqual([200, 200, 200, 200, 200, 429]);
	});
});

describe('language', () => {
	const signInHeading = async (res: Response) => (await res.text()).match(/<h1>(.*?)<\/h1>/)?.[1];

	it('follows the browser language and falls back to English', async () => {
		const open = (acceptLanguage: string) =>
			request('/account/', { headers: { 'accept-language': acceptLanguage } });
		expect(await signInHeading(await open('ja-JP,en;q=0.8'))).toBe('WebLAV にサインイン');
		expect(await signInHeading(await open('fr-FR,en;q=0.5,ja;q=0.9'))).toBe('WebLAV にサインイン');
		expect(await signInHeading(await open('en-US'))).toBe('Sign in to WebLAV');
		expect(await signInHeading(await open('fr-FR'))).toBe('Sign in to WebLAV');
	});

	it('keeps the language given in the link over the browser language', async () => {
		const res = await request('/account/transfer?lang=en');
		expect(await signInHeading(res)).toBe('Sign in to WebLAV');
		const cookie = res.headers.get('set-cookie')!.split(';')[0];
		expect(cookie).toBe('lang=en');
		expect(await signInHeading(await request('/account/', { cookie }))).toBe('Sign in to WebLAV');
	});

	it('sends the mail in the same language, with the language in the link', async () => {
		// WebLAV から開いたときに残した cookie で決まる。フォームの送り先には言語が付かない。
		const opened = await request('/account/transfer?lang=en');
		const cookie = opened.headers.get('set-cookie')!.split(';')[0];
		const log = vi.spyOn(console, 'log').mockImplementation(() => {});
		const sent = await postForm(
			'/account/login/email',
			{ email: 'english@example.com', next: '/account/' },
			cookie
		);
		expect(await sent.text()).toContain('Check your email');
		const mail = String(log.mock.calls.at(-1)?.[0]);
		expect(mail).toContain('Open the following link');
		expect(new URL(mail.match(/http\S+/)![0]).searchParams.get('lang')).toBe('en');
	});
});

describe('paths', () => {
	it('serves the account page with or without the trailing slash', async () => {
		for (const path of ['/account', '/account/']) {
			expect(await (await request(path)).text()).toContain('WebLAV にサインイン');
		}
	});
});

describe('footer', () => {
	it('links to the product page and its terms', async () => {
		const body = await (await request('/account/')).text();
		expect(body).toContain('href="/"');
		expect(body).toContain('href="/terms/"');
		expect(body).toContain('href="/privacy/"');
	});
});

describe('subscribing to Pro', () => {
	/** Stripe と同じ形の署名を付けて webhook を送る。 */
	async function webhook(event: object, { secret = 'whsec_test', t = now() } = {}) {
		const body = JSON.stringify(event);
		const key = await crypto.subtle.importKey(
			'raw',
			utf8(secret),
			{ name: 'HMAC', hash: 'SHA-256' },
			false,
			['sign']
		);
		const mac = new Uint8Array(await crypto.subtle.sign('HMAC', key, utf8(`${t}.${body}`)));
		const signature = toHex(mac);
		return request('/v1/stripe/webhook', {
			method: 'POST',
			headers: { 'content-type': 'application/json', 'stripe-signature': `t=${t},v1=${signature}` },
			body
		});
	}

	async function accountId(email: string) {
		const row = await env.DB.prepare('SELECT id FROM accounts WHERE email = ?')
			.bind(email)
			.first<{ id: string }>();
		return row!.id;
	}

	const DAY = 86_400;

	/** 知らせ。知らせの id は毎回変える (Stripe の送り直しを試すときは `id` に同じものを渡す)。 */
	function event(type: string, object: Record<string, unknown>, id = `evt_${crypto.randomUUID()}`) {
		return { id, type, data: { object } };
	}

	type Fake = {
		invoices: Record<string, Record<string, unknown>>;
		subscriptions: Record<string, Record<string, unknown>>;
		canceled: string[];
	};

	/** 請求書。払い終えた期間の終わりは `periodEnd`、払ったのは `pi_<id>`。 */
	function invoice(
		id: string,
		subscription: string,
		{ price = 'price_month', amount = 480, periodEnd = now() + 30 * DAY } = {}
	) {
		return {
			id,
			status: 'paid',
			currency: 'jpy',
			total: amount,
			amount_paid: amount,
			parent: { subscription_details: { subscription } },
			lines: {
				has_more: false,
				data: [
					{
						amount,
						quantity: 1,
						period: { start: periodEnd - 30 * DAY, end: periodEnd },
						pricing: { price_details: { price } },
						discount_amounts: []
					}
				]
			},
			payments: {
				has_more: false,
				data: [
					{
						status: 'paid',
						amount_paid: amount,
						payment: {
							type: 'payment_intent',
							payment_intent: {
								id: `pi_${id}`,
								latest_charge: `ch_${id}`
							}
						}
					}
				]
			}
		};
	}

	function subscription(id: string, metadata: Record<string, string>, status = 'active') {
		return { id, status, customer: `cus_${id}`, metadata, items: { data: [] } };
	}

	/** Stripe の API を、`fake` の中身を返すよう差し替える。 */
	function stripeApi(fake: Fake) {
		return vi.spyOn(globalThis, 'fetch').mockImplementation(async (url, init) => {
			const path = new URL(String(url)).pathname;
			const [, , kind, id] = path.split('/');
			if (kind === 'invoices') {
				// 本物と同じく、expand は4段までしか受け付けない。
				const tooDeep = new URL(String(url)).searchParams
					.getAll('expand[]')
					.some((e) => e.split('.').length > 4);
				if (tooDeep) return new Response('expand too deep', { status: 400 });
				return Response.json(fake.invoices[id]);
			}
			if (kind === 'charges') {
				return Response.json({ payment_method_details: { card: { country: 'JP' } } });
			}
			if (kind === 'subscriptions') {
				if (init?.method === 'DELETE') {
					fake.canceled.push(id);
					fake.subscriptions[id].status = 'canceled';
				}
				return Response.json(fake.subscriptions[id]);
			}
			if (kind === 'billing_portal') return Response.json({ url: 'https://billing.stripe.test/p' });
			return new Response('not found', { status: 404 });
		});
	}

	/** 個人向けを月額で申し込み、最初の請求書が払われたところ。 */
	async function personalSubscriber(email: string, fake: Fake, sub = `sub_${crypto.randomUUID()}`) {
		await signIn(email);
		fake.subscriptions[sub] = subscription(sub, {
			product: 'weblav-pro',
			account_id: await accountId(email),
			managed_payments: '0'
		});
		fake.invoices[`in_${sub}`] = invoice(`in_${sub}`, sub);
		return sub;
	}

	async function subscriptionOf(id: string) {
		return env.DB.prepare('SELECT * FROM subscriptions WHERE id = ?').bind(id).first<{
			account_id: string | null;
			plan: string;
			paid_through: number;
			status: string;
			revoked_at: number | null;
		}>();
	}

	function newFake(): Fake {
		return { invoices: {}, subscriptions: {}, canceled: [] };
	}

	it('sends the buyer to Stripe Checkout for a subscription', async () => {
		const { cookie } = await signIn('buyer@example.com');
		const stripe = vi
			.spyOn(globalThis, 'fetch')
			.mockImplementation(async () =>
				Response.json({ id: 'cs_1', url: 'https://checkout.stripe.test/c/pay/cs_1' })
			);
		const res = await postForm(
			'/account/buy',
			{ next: '/account/link?r=R&name=x', interval: 'year' },
			cookie
		);
		expect(res.status).toBe(303);
		expect(res.headers.get('location')).toBe('https://checkout.stripe.test/c/pay/cs_1');
		const [url, init] = stripe.mock.calls[0];
		expect(url).toBe('https://api.stripe.com/v1/checkout/sessions');
		const sent = new URLSearchParams(String(init!.body));
		expect(sent.get('mode')).toBe('subscription');
		expect(sent.get('line_items[0][price]')).toBe('price_year');
		expect(sent.get('subscription_data[metadata][account_id]')).toBe(
			await accountId('buyer@example.com')
		);
		expect(sent.get('subscription_data[metadata][product]')).toBe('weblav-pro');
		expect(sent.get('subscription_data[metadata][managed_payments]')).toBe('0');
		expect(sent.get('customer_email')).toBe('buyer@example.com');
		expect(sent.get('managed_payments[enabled]')).toBe('false');
		// 国内の分は、カードだけ・税率を付ける。
		expect(sent.get('payment_method_types[0]')).toBe('card');
		expect(sent.get('line_items[0][tax_rates][0]')).toBe(env.STRIPE_TAX_RATE_ID);
		expect(sent.get('locale')).toBe('ja');
		expect(sent.get('success_url')).toBe(
			`${ORIGIN}/account/buy/done?next=%2Faccount%2Flink%3Fr%3DR%26name%3Dx&lang=ja`
		);
		expect(sent.get('cancel_url')).toBe(`${ORIGIN}/account/link?r=R&name=x`);
		expect(sent.get('custom_text[submit][message]')).toContain(`${ORIGIN}/tokushoho/`);
		expect(sent.get('custom_text[submit][message]')).toContain('自動で更新');
	});

	it('does not sell again to an account that has Pro', async () => {
		const { cookie } = await signIn('owner@example.com');
		await grantPro('owner@example.com', 'personal');
		const stripe = vi.spyOn(globalThis, 'fetch');
		const again = await postForm('/account/buy', { next: '/account/' }, cookie);
		expect(again.status).toBe(303);
		expect(again.headers.get('location')).toBe('/account/');
		expect(stripe).not.toHaveBeenCalled();
	});

	it('sends the account page to the pricing page, which leads to a final review of the chosen plan', async () => {
		const { cookie } = await signIn('nopro@example.com');
		const home = await (await request('/account/', { cookie })).text();
		expect(home).toContain('href="/pricing/"');
		expect(home).not.toContain('action="/account/buy"');

		const next = '/account/link?r=R&name=x';
		const review = await (
			await request(`/account/buy?${new URLSearchParams({ plan: 'year', next })}`, { cookie })
		).text();
		// 最終確認画面に要る事項 (価格・自動の更新・解約・返金) と、規約類へのリンクをボタンの手前に出す。
		const button = review.indexOf('申し込みを確定して支払いへ');
		for (const term of [
			'お申し込み内容の最終確認',
			'4,800\u00a0円 / 年',
			'自動で更新',
			'解約',
			'href="/tokushoho/"'
		]) {
			expect(review.indexOf(term)).toBeGreaterThan(-1);
			expect(review.indexOf(term)).toBeLessThan(button);
		}
		expect(review).toContain('name="interval" value="year"');
		expect(review).toContain(`value="${next.replace(/&/g, '&amp;')}"`);
		expect(review).toContain(`href="/pricing/?${new URLSearchParams({ next })}"`);
		expect(review).not.toContain('Link');

		const bad = await request('/account/buy?plan=week', { cookie, redirect: 'manual' });
		expect(bad.status).toBe(303);
		expect(bad.headers.get('location')).toBe('/pricing/');
	});

	it('tells an account that already has Pro instead of reviewing an order', async () => {
		const { cookie } = await signIn('haspro@example.com');
		await grantPro('haspro@example.com', 'personal');
		const page = await (await request('/account/buy?plan=year', { cookie })).text();
		expect(page).toContain('このアカウントには Pro があります');
		expect(page).not.toContain('申し込みを確定して支払いへ');
	});

	it('lets an account order again once a canceled subscription has run out, without the grace', async () => {
		const email = 'switch@example.com';
		const { cookie } = await signIn(email);
		await grantPro(email, 'personal', now() - DAY);
		const review = () => request('/account/buy?plan=year', { cookie }).then((r) => r.text());
		// 払い直しを待っている間 (猶予の中) は、まだ Pro がある。
		expect(await review()).toContain('このアカウントには Pro があります');
		await env.DB.prepare(
			"UPDATE subscriptions SET status = 'canceled' WHERE account_id = (SELECT id FROM accounts WHERE email = ?)"
		)
			.bind(email)
			.run();
		expect(await review()).toContain('申し込みを確定して支払いへ');
	});

	it('brings back to the final review after signing in', async () => {
		const path = '/account/buy?plan=month&next=%2Faccount%2F';
		const before = await (await request(path)).text();
		expect(before).toContain('action="/account/login/email"');
		const { cookie, location } = await signIn('later@example.com', path);
		expect(location).toBe(path);
		expect(await (await request(path, { cookie })).text()).toContain('480\u00a0円 / 月');
	});

	it('sells through Managed Payments to buyers outside Japan', async () => {
		const { cookie } = await signIn('abroad@example.com');
		const review = await (
			await request('/account/buy?plan=month', { cookie, country: 'US' })
		).text();
		expect(review).toContain('Sold through Link, LLC');
		const stripe = vi
			.spyOn(globalThis, 'fetch')
			.mockImplementation(async () =>
				Response.json({ id: 'cs_abroad', url: 'https://checkout.stripe.test/abroad' })
			);
		const res = await postForm('/account/buy', { next: '/account/' }, cookie, { country: 'US' });
		expect(res.status).toBe(303);
		const sent = new URLSearchParams(String(stripe.mock.calls[0][1]!.body));
		expect(sent.get('managed_payments[enabled]')).toBe('true');
		expect(sent.get('subscription_data[metadata][managed_payments]')).toBe('1');
		// 払い方・税・支払いの画面の文言は MP が決めるので、送らない (送ると Stripe に断られる)。
		for (const key of [
			'custom_text[submit][message]',
			'payment_method_types[0]',
			'line_items[0][tax_rates][0]'
		]) {
			expect(sent.has(key)).toBe(false);
		}
	});

	it('sends back to the open checkout, and closes it when the other interval is chosen', async () => {
		const { cookie } = await signIn('twice@example.com');
		let n = 0;
		const calls: string[] = [];
		vi.spyOn(globalThis, 'fetch').mockImplementation(async (url) => {
			calls.push(new URL(String(url)).pathname);
			if (String(url).endsWith('/expire')) return Response.json({});
			n += 1;
			return Response.json({ id: `cs_twice${n}`, url: `https://checkout.stripe.test/${n}` });
		});
		const first = await postForm('/account/buy', { next: '/account/' }, cookie);
		const second = await postForm('/account/buy', { next: '/account/' }, cookie);
		expect(second.headers.get('location')).toBe(first.headers.get('location'));
		const yearly = await postForm('/account/buy', { next: '/account/', interval: 'year' }, cookie);
		expect(yearly.headers.get('location')).toBe('https://checkout.stripe.test/2');
		expect(calls).toEqual([
			'/v1/checkout/sessions',
			'/v1/checkout/sessions/cs_twice1/expire',
			'/v1/checkout/sessions'
		]);
	});

	it('asks Stripe with the same key while the checkout has no page, so it stays one', async () => {
		const { cookie } = await signIn('race@example.com');
		const keys: string[] = [];
		const bodies: string[] = [];
		let reply = () => new Response('{"error":{"type":"idempotency_error"}}', { status: 409 });
		vi.spyOn(globalThis, 'fetch').mockImplementation(async (_url, init) => {
			keys.push(new Headers(init!.headers).get('idempotency-key')!);
			bodies.push(String(init!.body));
			return reply();
		});
		// ほかのタブが頼んでいる最中: 予約は残し、押し直してもらう。
		const busy = await postForm('/account/buy', { next: '/account/transfer' }, cookie);
		expect(busy.status).toBe(409);
		reply = () => Response.json({ id: 'cs_race', url: 'https://checkout.stripe.test/race' });
		// 押し直しは、戻り先が違っても最初の予約と同じ頼みにする (Stripe は中身の違う頼み直しを断る)。
		const again = await postForm('/account/buy', { next: '/account/' }, cookie);
		expect(again.headers.get('location')).toBe('https://checkout.stripe.test/race');
		expect(keys[1]).toBe(keys[0]);
		expect(bodies[1]).toBe(bodies[0]);
	});

	it('forgets the checkout when Stripe fails, so the next press starts over', async () => {
		const { cookie } = await signIn('fail@example.com');
		const keys: string[] = [];
		let reply = () => new Response('{"error":{"type":"api_error"}}', { status: 500 });
		vi.spyOn(globalThis, 'fetch').mockImplementation(async (_url, init) => {
			keys.push(new Headers(init!.headers).get('idempotency-key')!);
			return reply();
		});
		expect((await postForm('/account/buy', { next: '/account/' }, cookie)).status).toBe(500);
		reply = () => Response.json({ id: 'cs_fail', url: 'https://checkout.stripe.test/fail' });
		const again = await postForm('/account/buy', { next: '/account/' }, cookie);
		expect(again.headers.get('location')).toBe('https://checkout.stripe.test/fail');
		expect(keys[1]).not.toBe(keys[0]);
	});

	it('asks again with the same key when no answer came back from Stripe', async () => {
		const { cookie } = await signIn('lost@example.com');
		const keys: string[] = [];
		let reply = (): Response => {
			throw new TypeError('network connection lost');
		};
		vi.spyOn(globalThis, 'fetch').mockImplementation(async (_url, init) => {
			keys.push(new Headers(init!.headers).get('idempotency-key')!);
			return reply();
		});
		expect((await postForm('/account/buy', { next: '/account/' }, cookie)).status).toBe(500);
		reply = () => Response.json({ id: 'cs_lost', url: 'https://checkout.stripe.test/lost' });
		const again = await postForm('/account/buy', { next: '/account/' }, cookie);
		expect(again.headers.get('location')).toBe('https://checkout.stripe.test/lost');
		expect(keys[1]).toBe(keys[0]);
	});

	it('grants Pro from the first paid invoice once, however often Stripe resends it', async () => {
		const fake = newFake();
		const sub = await personalSubscriber('paid@example.com', fake);
		stripeApi(fake);
		const paid = event('invoice.paid', { id: `in_${sub}` });
		expect((await webhook(paid)).status).toBe(200);
		expect((await webhook(paid)).status).toBe(200);
		expect((await webhook(event('invoice.paid', { id: `in_${sub}` }))).status).toBe(200);
		const row = await subscriptionOf(sub);
		expect(row).toMatchObject({
			account_id: await accountId('paid@example.com'),
			plan: 'personal',
			status: 'active',
			revoked_at: null
		});
		expect(row!.paid_through).toBe(
			(fake.invoices[`in_${sub}`].lines as { data: { period: { end: number } }[] }).data[0].period
				.end
		);
		const { results } = await env.DB.prepare(
			'SELECT amount, managed_payments, card_country FROM purchases WHERE stripe_subscription_id = ?'
		)
			.bind(sub)
			.all();
		expect(results).toEqual([{ amount: 480, managed_payments: 0, card_country: 'JP' }]);
	});

	it('extends the paid period with each invoice and never shortens it', async () => {
		const fake = newFake();
		const sub = await personalSubscriber('renew@example.com', fake);
		stripeApi(fake);
		await webhook(event('invoice.paid', { id: `in_${sub}` }));
		const later = now() + 60 * DAY;
		fake.invoices.in_next = invoice('in_next', sub, { periodEnd: later });
		await webhook(event('invoice.paid', { id: 'in_next' }));
		expect((await subscriptionOf(sub))!.paid_through).toBe(later);
		// 遅れて届いた古い請求書で縮めない。
		fake.invoices.in_old = invoice('in_old', sub, { periodEnd: now() + 10 * DAY });
		await webhook(event('invoice.paid', { id: 'in_old' }));
		expect((await subscriptionOf(sub))!.paid_through).toBe(later);
	});

	it('ignores invoices of other products and those without a subscription', async () => {
		const fake = newFake();
		const sub = await personalSubscriber('other@example.com', fake);
		fake.invoices.in_other = invoice('in_other', sub, { price: 'price_bp_carnet' });
		fake.invoices.in_single = { ...invoice('in_single', sub), parent: null };
		stripeApi(fake);
		await webhook(event('invoice.paid', { id: 'in_other' }));
		await webhook(event('invoice.paid', { id: 'in_single' }));
		expect(await subscriptionOf(sub)).toBeNull();
	});

	// 署名の場合分け (古い時刻など) は stripe.test.ts で確かめる。
	it('rejects webhooks with a wrong signature', async () => {
		const res = await webhook(event('invoice.paid', { id: 'in_x' }), { secret: 'whsec_wrong' });
		expect(res.status).toBe(400);
		expect(await res.json()).toEqual({ error: 'invalid_signature' });
	});

	it('cuts the subscription off on a full refund and a dispute, and never extends it again', async () => {
		for (const [type, object] of [
			['charge.refunded', { refunded: true }],
			['charge.dispute.created', {}]
		] as const) {
			const fake = newFake();
			const sub = await personalSubscriber(`cut-${type}@example.com`, fake);
			stripeApi(fake);
			await webhook(event('invoice.paid', { id: `in_${sub}` }));
			await webhook(event(type, { payment_intent: `pi_in_${sub}`, ...object }));
			const row = await subscriptionOf(sub);
			expect(row!.revoked_at).not.toBeNull();
			expect(fake.canceled).toEqual([sub]);
			fake.invoices.in_late = invoice('in_late', sub, { periodEnd: now() + 90 * DAY });
			await webhook(event('invoice.paid', { id: 'in_late' }));
			expect((await subscriptionOf(sub))!.paid_through).toBe(row!.paid_through);
			const ledger = await env.DB.prepare(
				'SELECT revoked_at FROM purchases WHERE stripe_invoice_id = ?'
			)
				.bind(`in_${sub}`)
				.first<{ revoked_at: number | null }>();
			expect(ledger!.revoked_at).not.toBeNull();
		}
	});

	it('does not take Pro away on a partial refund', async () => {
		const fake = newFake();
		const sub = await personalSubscriber('partial@example.com', fake);
		stripeApi(fake);
		await webhook(event('invoice.paid', { id: `in_${sub}` }));
		await webhook(event('charge.refunded', { payment_intent: `pi_in_${sub}`, refunded: false }));
		expect((await subscriptionOf(sub))!.revoked_at).toBeNull();
	});

	it('does not grant an invoice whose refund arrived first, and cancels it', async () => {
		const fake = newFake();
		const sub = await personalSubscriber('refund-first@example.com', fake);
		stripeApi(fake);
		await webhook(event('charge.refunded', { payment_intent: `pi_in_${sub}`, refunded: true }));
		await webhook(event('invoice.paid', { id: `in_${sub}` }));
		expect(await subscriptionOf(sub)).toBeNull();
		expect(fake.canceled).toEqual([sub]);
		const ledger = await env.DB.prepare(
			'SELECT revoked_at FROM purchases WHERE stripe_invoice_id = ?'
		)
			.bind(`in_${sub}`)
			.first<{ revoked_at: number | null }>();
		expect(ledger!.revoked_at).not.toBeNull();
	});

	it('follows the status from Stripe, and never brings back a canceled one', async () => {
		const fake = newFake();
		const sub = await personalSubscriber('status@example.com', fake);
		stripeApi(fake);
		await webhook(event('invoice.paid', { id: `in_${sub}` }));
		fake.subscriptions[sub].status = 'past_due';
		// 本文の状態は信じない (取り直した状態で合わせる)。
		await webhook(event('customer.subscription.updated', { id: sub, status: 'active' }));
		expect((await subscriptionOf(sub))!.status).toBe('past_due');
		fake.subscriptions[sub].status = 'canceled';
		await webhook(event('customer.subscription.deleted', { id: sub }));
		fake.subscriptions[sub].status = 'active';
		await webhook(event('customer.subscription.updated', { id: sub }));
		expect((await subscriptionOf(sub))!.status).toBe('canceled');
	});

	it('keeps Pro until the end of the paid period plus the grace, then stops', async () => {
		const email = 'grace@example.com';
		const { cookie } = await signIn(email);
		await grantPro(email, 'personal', now() - 6 * DAY);
		const req = await makeRequest();
		await link(cookie, req.raw);
		expect((await (await check(req.secret)).json<{ status: string }>()).status).toBe('ok');
		await env.DB.prepare(
			'UPDATE subscriptions SET paid_through = ? WHERE account_id = (SELECT id FROM accounts WHERE email = ?)'
		)
			.bind(now() - 8 * DAY, email)
			.run();
		expect(await (await check(req.secret)).json()).toEqual({ status: 'no_plan', email });
	});

	it('keeps the ledger and the subscription row without the account when the account is deleted', async () => {
		const fake = newFake();
		const sub = await personalSubscriber('leaving@example.com', fake);
		stripeApi(fake);
		await webhook(event('invoice.paid', { id: `in_${sub}` }));
		const { cookie } = await signIn('leaving@example.com');
		await env.DB.batch(
			DELETE_ACCOUNT_STATEMENTS.map((s) => env.DB.prepare(s).bind('leaving@example.com'))
		);
		expect((await subscriptionOf(sub))!.account_id).toBeNull();
		const row = await env.DB.prepare(
			'SELECT account_id, amount, managed_payments, card_country FROM purchases WHERE stripe_invoice_id = ?'
		)
			.bind(`in_${sub}`)
			.first();
		expect(row).toEqual({
			account_id: null,
			amount: 480,
			managed_payments: null,
			card_country: null
		});
		// 後から届いた知らせで、Pro を付け直さない。
		fake.invoices.in_after = invoice('in_after', sub, { periodEnd: now() + 90 * DAY });
		await webhook(event('invoice.paid', { id: 'in_after' }));
		expect((await subscriptionOf(sub))!.account_id).toBeNull();
		// サインインの状態も消える。
		expect(await (await request('/account/', { cookie })).text()).toContain(
			'action="/account/login/email"'
		);
	});

	it('does not create the row when the first invoice comes after the account was deleted', async () => {
		const fake = newFake();
		const sub = await personalSubscriber('gone@example.com', fake);
		await env.DB.batch(
			DELETE_ACCOUNT_STATEMENTS.map((s) => env.DB.prepare(s).bind('gone@example.com'))
		);
		stripeApi(fake);
		await webhook(event('invoice.paid', { id: `in_${sub}` }));
		expect(await subscriptionOf(sub)).toBeNull();
		expect(
			await env.DB.prepare('SELECT 1 FROM purchases WHERE stripe_invoice_id = ?')
				.bind(`in_${sub}`)
				.first()
		).not.toBeNull();
	});

	it('forgets purchases of deleted accounts after the retention period', async () => {
		const fake = newFake();
		const sub = await personalSubscriber('old-buyer@example.com', fake);
		stripeApi(fake);
		await webhook(event('invoice.paid', { id: `in_${sub}` }));
		await env.DB.batch(
			DELETE_ACCOUNT_STATEMENTS.map((s) => env.DB.prepare(s).bind('old-buyer@example.com'))
		);
		const eightYearsAgo = now() - 8 * 366 * DAY;
		const exists = () =>
			env.DB.prepare('SELECT 1 FROM purchases WHERE stripe_invoice_id = ?')
				.bind(`in_${sub}`)
				.first();
		// 買ったのが昔でも、消してから7年は残す。
		await env.DB.prepare('UPDATE purchases SET created_at = ? WHERE stripe_invoice_id = ?')
			.bind(eightYearsAgo, `in_${sub}`)
			.run();
		await webhook(event('customer.created', {}));
		expect(await exists()).not.toBeNull();
		await env.DB.prepare('UPDATE purchases SET detached_at = ? WHERE stripe_invoice_id = ?')
			.bind(eightYearsAgo, `in_${sub}`)
			.run();
		await webhook(event('customer.created', {}));
		expect(await exists()).toBeNull();
	});

	it('goes on once Pro has arrived, and keeps checking until then', async () => {
		const fake = newFake();
		const sub = await personalSubscriber('back@example.com', fake);
		const { cookie } = await signIn('back@example.com');
		const path = '/account/buy/done?next=%2Faccount%2Ftransfer';
		const waiting = await request(path, { cookie });
		expect(waiting.status).toBe(200);
		expect(await waiting.text()).toContain('http-equiv="refresh"');
		const gaveUp = await (await request(`${path}&tries=10`, { cookie })).text();
		expect(gaveUp).not.toContain('http-equiv="refresh"');
		expect(gaveUp).toContain('tries=11');
		stripeApi(fake);
		await webhook(event('invoice.paid', { id: `in_${sub}` }));
		const done = await request(path, { cookie, redirect: 'manual' });
		expect(done.status).toBe(303);
		expect(done.headers.get('location')).toBe('/account/transfer');
	});

	it('opens the customer portal for an account with a Stripe subscription', async () => {
		const fake = newFake();
		const sub = await personalSubscriber('portal@example.com', fake);
		const stripe = stripeApi(fake);
		await webhook(event('invoice.paid', { id: `in_${sub}` }));
		const { cookie } = await signIn('portal@example.com');
		expect(await (await request('/account/', { cookie })).text()).toContain(
			'action="/account/billing"'
		);
		const res = await postForm('/account/billing', {}, cookie);
		expect(res.status).toBe(303);
		expect(res.headers.get('location')).toBe('https://billing.stripe.test/p');
		const call = stripe.mock.calls.find(([u]) => String(u).includes('billing_portal'))!;
		expect(new URLSearchParams(String(call[1]!.body)).get('customer')).toBe(`cus_${sub}`);
	});

	describe('organization', () => {
		function orgSubscription(fake: Fake, email: string, sub = `sub_${crypto.randomUUID()}`) {
			fake.subscriptions[sub] = subscription(sub, { product: 'weblav-org', account_email: email });
			fake.invoices[`in_${sub}`] = invoice(`in_${sub}`, sub, {
				price: 'price_org',
				amount: 14800,
				periodEnd: now() + 365 * DAY
			});
			return sub;
		}

		it('grants organization Pro to the account in the metadata, creating it', async () => {
			const fake = newFake();
			const sub = orgSubscription(fake, 'School@Example.com');
			stripeApi(fake);
			await webhook(event('invoice.paid', { id: `in_${sub}` }));
			expect(await subscriptionOf(sub)).toMatchObject({
				account_id: await accountId('school@example.com'),
				plan: 'organization'
			});
		});

		it('keeps renewals with the new owner after moving Pro', async () => {
			const fake = newFake();
			const sub = orgSubscription(fake, 'office@example.com');
			stripeApi(fake);
			await webhook(event('invoice.paid', { id: `in_${sub}` }));
			const { cookie } = await signIn('office@example.com');
			vi.spyOn(console, 'log').mockImplementation(() => {});
			await postForm('/account/transfer', { email: 'next@example.com', confirm: '1' }, cookie);
			const year2 = now() + 730 * DAY;
			fake.invoices.in_year2 = invoice('in_year2', sub, {
				price: 'price_org',
				amount: 14800,
				periodEnd: year2
			});
			await webhook(event('invoice.paid', { id: 'in_year2' }));
			expect(await subscriptionOf(sub)).toMatchObject({
				account_id: await accountId('next@example.com'),
				paid_through: year2
			});
		});

		it('does not grant a subscription without the product mark', async () => {
			const fake = newFake();
			const sub = orgSubscription(fake, 'unmarked@example.com');
			fake.subscriptions[sub] = subscription(sub, { account_email: 'unmarked@example.com' });
			stripeApi(fake);
			vi.spyOn(console, 'log').mockImplementation(() => {});
			await webhook(event('invoice.paid', { id: `in_${sub}` }));
			expect(await subscriptionOf(sub)).toBeNull();
		});

		it('tells the operator when the metadata has no address, and asks Stripe to retry later', async () => {
			const fake = newFake();
			const sub = orgSubscription(fake, 'not an address');
			stripeApi(fake);
			const mails: string[] = [];
			vi.spyOn(console, 'log').mockImplementation((line: string) => {
				mails.push(line);
			});
			const paid = event('invoice.paid', { id: `in_${sub}` });
			expect((await webhook(paid)).status).toBe(200);
			expect(await subscriptionOf(sub)).toBeNull();
			expect(mails.some((m) => m.includes('to=support@amiiby.com'))).toBe(true);
			const status = await env.DB.prepare('SELECT status FROM stripe_events WHERE id = ?')
				.bind(paid.id)
				.first<{ status: string }>();
			expect(status!.status).toBe('failed');
		});
	});
});
