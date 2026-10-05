/**
 * WebLAV の結び付きの符号 (→ docs/pro.md「符号の作り方」)。確かめる側は WebLAV の `src/pro/link.rs`。
 * 秘密は D1 に持たず、WebLAV の公開鍵と窓口の X25519 の秘密鍵から、要るたびに導き直す。
 */
import { base64url, base64urlBytes, fromHex, toHex, utf8 } from './util';

export type Plan = 'personal' | 'organization';

/** 申し込みの版。形を変えるときに上げる。 */
const REQUEST_VERSION = 1;
/** 申し込みを受け付ける長さ。古い QR コードを後から読んで、使われない枠を作らないように。 */
export const REQUEST_TTL = 24 * 60 * 60;
/** 返しのコードの日付の起点 (2026-01-01 UTC)。16ビットで約179年分を数えられる。 */
const EPOCH_DAY = Date.UTC(2026, 0, 1) / 86_400_000;
/** 発行日から期限までの日数を入れるビット数。年額 (366日) に組織向けの猶予 (30日) を足しても収まる。 */
const SPAN_BITS = 9;
/** 返しのコードの符号の長さ (ビット)。打ち間違いを通さず、15文字に収まる長さ。 */
const GRANT_TAG_BITS = 49;

const CROCKFORD = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';

/** 5ビットずつ Crockford の base32 にする。末尾の足りないビットは 0 で埋める。 */
export function encodeBase32(bytes: Uint8Array, bits = bytes.length * 8): string {
	let out = '';
	for (let i = 0; i < bits; i += 5) {
		let v = 0;
		for (let j = 0; j < 5; j++) v = (v << 1) | bit(bytes, i + j);
		out += CROCKFORD[v];
	}
	return out;
}

/** 打ち込まれた base32 を読む。区切り・空白を除き、`O`→`0`・`I`/`L`→`1` に読み替える。読めなければ `undefined`。 */
export function decodeBase32(text: string, bits: number): Uint8Array | undefined {
	const chars = normalizeBase32(text);
	if (chars.length !== Math.ceil(bits / 5)) return undefined;
	const out = new Uint8Array(Math.ceil(bits / 8));
	let pos = 0;
	for (const ch of chars) {
		const v = CROCKFORD.indexOf(ch);
		if (v < 0) return undefined;
		for (let j = 4; j >= 0; j--) {
			if (pos < bits) setBit(out, pos, (v >> j) & 1);
			else if ((v >> j) & 1) return undefined; // 埋めたビットが 0 でないものは受けない
			pos++;
		}
	}
	return out;
}

function normalizeBase32(text: string): string {
	return text.toUpperCase().replace(/[\s-]/g, '').replace(/O/g, '0').replace(/[IL]/g, '1');
}

function bit(bytes: Uint8Array, i: number): number {
	return i < bytes.length * 8 ? (bytes[i >> 3] >> (7 - (i & 7))) & 1 : 0;
}

function setBit(bytes: Uint8Array, i: number, v: number) {
	if (v) bytes[i >> 3] |= 1 << (7 - (i & 7));
}

function concat(...parts: Uint8Array[]): Uint8Array {
	const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
	let at = 0;
	for (const p of parts) {
		out.set(p, at);
		at += p.length;
	}
	return out;
}

async function hmac(key: Uint8Array, data: Uint8Array): Promise<Uint8Array> {
	const k = await crypto.subtle.importKey('raw', key, { name: 'HMAC', hash: 'SHA-256' }, false, [
		'sign'
	]);
	return new Uint8Array(await crypto.subtle.sign('HMAC', k, data));
}

/** 定数時間で比べる (Workers の `timingSafeEqual`)。長さが違えば false。 */
export function sameBytes(a: Uint8Array, b: Uint8Array): boolean {
	return a.length === b.length && crypto.subtle.timingSafeEqual(a, b);
}

/** 窓口の X25519 の鍵。`LINK_KEY` は JWK (`crv: X25519`)、`LINK_KID` は 0〜255。 */
export type LinkKey = { kid: number; privateKey: CryptoKey; publicKey: Uint8Array };

export async function linkKey(env: Env): Promise<LinkKey> {
	const jwk = JSON.parse(env.LINK_KEY) as JsonWebKey & { x: string };
	const privateKey = await crypto.subtle.importKey('jwk', jwk, { name: 'X25519' }, false, [
		'deriveBits'
	]);
	return {
		kid: Number(env.LINK_KID),
		privateKey,
		publicKey: base64urlBytes(jwk.x)
	};
}

/** WebLAV の公開鍵から秘密を導く。共有の値が全部 0 (小さい位数の点) なら `undefined`。 */
export async function deriveSecret(
	key: LinkKey,
	clientPublic: Uint8Array
): Promise<Uint8Array | undefined> {
	if (clientPublic.length !== 32) return undefined;
	let shared: Uint8Array;
	try {
		const pub = await crypto.subtle.importKey('raw', clientPublic, { name: 'X25519' }, false, []);
		shared = new Uint8Array(
			await crypto.subtle.deriveBits(
				// FIX: 生成した型 (worker-configuration.d.ts) では `public` が `$public` になっている。
				// 動くのは Web Crypto の決まりどおりの `public` (テストで確かめている)。
				{ name: 'X25519', public: pub } as unknown as SubtleCryptoDeriveKeyAlgorithm,
				key.privateKey,
				256
			)
		);
	} catch {
		return undefined;
	}
	if (shared.every((b) => b === 0)) return undefined;
	const ikm = await crypto.subtle.importKey('raw', shared, 'HKDF', false, ['deriveBits']);
	return new Uint8Array(
		await crypto.subtle.deriveBits(
			{
				name: 'HKDF',
				hash: 'SHA-256',
				salt: utf8('weblav-link-v1'),
				info: concat(new Uint8Array([key.kid]), clientPublic, key.publicKey)
			},
			ikm,
			256
		)
	);
}

export async function installationId(secret: Uint8Array): Promise<string> {
	return toHex((await hmac(secret, utf8('weblav-installation-id'))).slice(0, 8));
}

export async function checkAuth(secret: Uint8Array): Promise<Uint8Array> {
	return hmac(secret, utf8('weblav-check'));
}

export async function relinkTag(previousSecret: Uint8Array, newPublic: Uint8Array) {
	return (await hmac(previousSecret, concat(utf8('weblav-relink'), newPublic))).slice(0, 8);
}

export async function releaseTag(secret: Uint8Array) {
	return (await hmac(secret, utf8('weblav-release'))).slice(0, 6);
}

/** 読み取った申し込み。 */
export type LinkRequest = {
	kid: number;
	createdAt: number;
	publicKey: Uint8Array;
	/** 同じ WebLAV の結び直しのときだけ。 */
	relink?: { installation: string; tag: Uint8Array };
};

const REQUEST_BYTES = 1 + 1 + 4 + 32;
const RELINK_BYTES = 8 + 8;

/** 申し込みの文字列を読む。形が違えば `undefined`。期限は呼ぶ側で見る。 */
export function parseRequest(text: string): LinkRequest | undefined {
	const chars = normalizeBase32(text).length;
	for (const length of [REQUEST_BYTES, REQUEST_BYTES + RELINK_BYTES]) {
		if (chars !== Math.ceil((length * 8) / 5)) continue;
		const bytes = decodeBase32(text, length * 8);
		if (!bytes || bytes[0] !== REQUEST_VERSION) return undefined;
		const view = new DataView(bytes.buffer);
		const request: LinkRequest = {
			kid: bytes[1],
			createdAt: view.getUint32(2) * 60,
			publicKey: bytes.slice(6, 38)
		};
		if (length > REQUEST_BYTES) {
			request.relink = { installation: toHex(bytes.slice(38, 46)), tag: bytes.slice(46, 54) };
		}
		return request;
	}
	return undefined;
}

/** テストと WebLAV の手元の窓口の確かめ用に、申し込みを作る (WebLAV の作り方と同じ)。 */
export function formatRequest(request: LinkRequest): string {
	const head = new Uint8Array(6);
	head[0] = REQUEST_VERSION;
	head[1] = request.kid;
	new DataView(head.buffer).setUint32(2, Math.floor(request.createdAt / 60));
	const parts = [head, request.publicKey];
	if (request.relink) parts.push(fromHex(request.relink.installation)!, request.relink.tag);
	return encodeBase32(concat(...parts));
}

/** 返しのコード (15文字)。期限は発行日の終わりから数えた日数で表す。 */
export async function grantCode(
	secret: Uint8Array,
	plan: Plan,
	issuedAt: number,
	expiresAt: number
): Promise<string> {
	const issuedDay = Math.floor(issuedAt / 86_400) - EPOCH_DAY;
	const span = Math.min(
		Math.floor(expiresAt / 86_400) - EPOCH_DAY - issuedDay,
		(1 << SPAN_BITS) - 1
	);
	const head = ((plan === 'organization' ? 1 : 0) << 25) | (issuedDay << SPAN_BITS) | span;
	const headBytes = new Uint8Array(4);
	new DataView(headBytes.buffer).setUint32(0, head);
	const tag = await hmac(secret, concat(utf8('weblav-grant'), headBytes));
	// 26ビットの頭と、符号の先頭49ビットを並べた75ビット。
	const out = new Uint8Array(10);
	for (let i = 0; i < 26; i++) setBit(out, i, bit(headBytes, 6 + i));
	for (let i = 0; i < GRANT_TAG_BITS; i++) setBit(out, 26 + i, bit(tag, i));
	const code = encodeBase32(out, 26 + GRANT_TAG_BITS);
	return `${code.slice(0, 5)}-${code.slice(5, 10)}-${code.slice(10)}`;
}

/** 返しのコードの期限 (UNIX 秒)。発行日から数えた日の終わり。 */
export function grantExpiresAt(issuedAt: number, expiresAt: number): number {
	const issuedDay = Math.floor(issuedAt / 86_400);
	const span = Math.min(Math.floor(expiresAt / 86_400) - issuedDay, (1 << SPAN_BITS) - 1);
	return (issuedDay + span + 1) * 86_400;
}

/** 外した証し (23文字) を読む。形が違えば `undefined`。 */
export function parseRelease(text: string): { installation: string; tag: Uint8Array } | undefined {
	const bytes = decodeBase32(text, 14 * 8);
	if (!bytes) return undefined;
	return { installation: toHex(bytes.slice(0, 8)), tag: bytes.slice(8, 14) };
}

export async function formatRelease(secret: Uint8Array): Promise<string> {
	const id = fromHex(await installationId(secret))!;
	return encodeBase32(concat(id, await releaseTag(secret)));
}

/** Ed25519 の証明 (→ docs/pro.md「結び付きと許可」)。確かめる側は WebLAV の `src/pro.rs`。 */
export async function signProof(
	env: Env,
	claims: { account: string; installation: string; plan: Plan; issuedAt: number; expiresAt: number }
): Promise<string> {
	const payload = utf8(
		JSON.stringify({
			v: 2,
			proof_kid: env.PROOF_KID,
			account: claims.account,
			installation: claims.installation,
			plan: claims.plan,
			issued_at: claims.issuedAt,
			expires_at: claims.expiresAt
		})
	);
	const key = await crypto.subtle.importKey(
		'jwk',
		JSON.parse(env.PROOF_SIGNING_KEY),
		{ name: 'Ed25519' },
		false,
		['sign']
	);
	const signature = await crypto.subtle.sign('Ed25519', key, payload);
	return `${base64url(payload)}.${base64url(new Uint8Array(signature))}`;
}
