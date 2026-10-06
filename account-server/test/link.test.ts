import { env } from 'cloudflare:workers';
import { describe, expect, it } from 'vitest';
import {
	checkAuth,
	decodeBase32,
	deriveSecret,
	encodeBase32,
	formatRelease,
	formatRequest,
	grantCode,
	grantExpiresAt,
	installationId,
	linkKey,
	parseRelease,
	parseRequest,
	relinkTag,
	signProof
} from '../src/link';
import { fromHex, toHex } from '../src/util';

/**
 * WebLAV (`src/pro/link.rs` の `matches_the_account_server_vectors`) と同じ入力から、同じ値が出ること。
 * WebLAV の秘密鍵は 7 を32個並べたもの、窓口の鍵は dev.vars.example の `LINK_KEY`。
 */
const VECTORS = {
	public: '13be4feaeaf204c7fd3358fc9c00721881d174278128227ec674f37f7fe97b6d',
	secret: '1ace5b59d960e76fbb90933fbf772723d51f15fd6649c90ff2d616fe1f53c4fc',
	id: '9d1e343f38716e3c',
	auth: '428e868e000884ee74ca2eed88e121ac8a173ab4962a56866a79bd5d64d53925',
	request: '04003HVZDG9VWKZAXBS09HZX6DCFS700E8C83MBM4Y0JG8KYRSTF6ZVZX5XPT',
	relink: '04003HVZDG9VWKZAXBS09HZX6DCFS700E8C83MBM4Y0JG8KYRSTF6ZVZX5XPTKA3SRZHADTDFY68F1ST9NXK7X8',
	release: 'KMF38FSRE5Q3RN0RHA5SMCR',
	// 下の `grant` の入力で作った返しのコード。WebLAV の `parses_the_account_server_grant` が読む。
	grant: 'G250F-CMYG3-D78AW'
};

describe('link codes', () => {
	it('derives the same secret and values as WebLAV', async () => {
		const key = await linkKey(env);
		const secret = (await deriveSecret(key, fromHex(VECTORS.public)!))!;
		expect(toHex(secret)).toBe(VECTORS.secret);
		expect(await installationId(secret)).toBe(VECTORS.id);
		expect(toHex(await checkAuth(secret))).toBe(VECTORS.auth);
		expect(await formatRelease(secret)).toBe(VECTORS.release);
		const request = parseRequest(VECTORS.request)!;
		expect(request).toEqual({
			kid: 0,
			createdAt: 1_791_090_000 - 0,
			publicKey: fromHex(VECTORS.public)
		});
		const previous = new Uint8Array(32).fill(9);
		const relink = parseRequest(VECTORS.relink)!;
		expect(relink.relink).toEqual({
			installation: await installationId(previous),
			tag: await relinkTag(previous, fromHex(VECTORS.public)!)
		});
		expect(formatRequest(relink)).toBe(VECTORS.relink);
		expect(parseRelease(VECTORS.release)).toEqual({
			installation: VECTORS.id,
			tag: parseRelease(VECTORS.release)!.tag
		});
	});

	it('makes the grant code that WebLAV reads', async () => {
		const secret = fromHex(VECTORS.secret)!;
		const issued = 1_791_090_000;
		expect(await grantCode(secret, 'organization', issued, issued + 30 * 86_400)).toBe(
			VECTORS.grant
		);
		// 期限は、期限の日の終わり (UTC)。
		expect(grantExpiresAt(issued, issued + 30 * 86_400)).toBe(
			(Math.floor(issued / 86_400) + 31) * 86_400
		);
	});

	it('reads base32 loosely but rejects other lengths and stray bits', () => {
		const bytes = Uint8Array.from([0xde, 0xad, 0xbe, 0xef, 0x01]);
		const text = encodeBase32(bytes);
		const loose = text.toLowerCase().replace(/0/g, 'o').replace(/1/g, 'l');
		expect(decodeBase32(`${loose.slice(0, 4)}-${loose.slice(4)}`, 40)).toEqual(bytes);
		expect(decodeBase32(text.slice(1), 40)).toBeUndefined();
		// 埋めたビットが 0 でない
		expect(decodeBase32('ZZ', 8)).toBeUndefined();
	});

	it('refuses a small-order public key', async () => {
		expect(await deriveSecret(await linkKey(env), new Uint8Array(32))).toBeUndefined();
	});
});

describe('signProof', () => {
	it('signs with a key exported by Node, which adds alg "Ed25519"', async () => {
		// 本番の鍵は scripts/new-proof-key.mjs が Node 24 で作り、この形 (key_ops・ext・alg が付く) で置かれている。
		const jwk = {
			...JSON.parse(env.PROOF_SIGNING_KEY),
			key_ops: ['sign'],
			ext: true,
			alg: 'Ed25519'
		};
		const proof = await signProof(
			{ ...env, PROOF_SIGNING_KEY: JSON.stringify(jwk) },
			{ account: 'a', installation: 'i', plan: 'personal', issuedAt: 1, expiresAt: 2 }
		);
		const [payload, signature] = proof.split('.');
		const bytes = (s: string) =>
			Uint8Array.from(atob(s.replace(/-/g, '+').replace(/_/g, '/')), (c) => c.charCodeAt(0));
		const key = await crypto.subtle.importKey(
			'jwk',
			{ kty: jwk.kty, crv: jwk.crv, x: jwk.x },
			{ name: 'Ed25519' },
			false,
			['verify']
		);
		expect(await crypto.subtle.verify('Ed25519', key, bytes(signature), bytes(payload))).toBe(true);
	});
});
