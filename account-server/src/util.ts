/** 今の時刻 (UNIX 秒)。 */
export function now(): number {
	return Math.floor(Date.now() / 1000);
}

/** 推測できない乱数を16進の文字列で。 */
export function randomHex(bytes: number): string {
	return toHex(crypto.getRandomValues(new Uint8Array(bytes)));
}

/** トークンや秘密は、この値だけを D1 に持つ。D1 が漏れても使えないように。 */
export async function sha256Hex(text: string): Promise<string> {
	const digest = await crypto.subtle.digest('SHA-256', utf8(text));
	return toHex(new Uint8Array(digest));
}

export function utf8(text: string): Uint8Array {
	return new TextEncoder().encode(text);
}

export function toHex(bytes: Uint8Array): string {
	return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

/** 小文字の16進を読む。形が違えば `undefined`、空の文字列は空のバイト列。 */
export function fromHex(hex: string): Uint8Array | undefined {
	if (!/^(?:[0-9a-f]{2})*$/.test(hex)) return undefined;
	return Uint8Array.from(hex.match(/../g) ?? [], (b) => parseInt(b, 16));
}

/** フォームの値のうち、文字列のものだけを取り出す (ファイルは除く)。 */
export function formString(form: Record<string, unknown>, key: string): string | undefined {
	const value = form[key];
	return typeof value === 'string' ? value : undefined;
}

/** https で受けたか。Cookie の `Secure` と `__Host-` を付けるかを決める。 */
export function isHttps(c: { req: { url: string } }): boolean {
	return new URL(c.req.url).protocol === 'https:';
}

/** 受けた要求のオリジン。メールや Stripe に渡す、このサイトへ戻る URL を組み立てる。 */
export function originOf(c: { req: { url: string } }): string {
	return new URL(c.req.url).origin;
}

/** 送れそうな形か。確かめるのはメールが届くことでする。 */
export function isEmail(email: string): boolean {
	return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email);
}

export function normalizeEmail(email: string): string {
	return email.trim().toLowerCase();
}

export function base64url(bytes: Uint8Array): string {
	return btoa(String.fromCharCode(...bytes))
		.replaceAll('+', '-')
		.replaceAll('/', '_')
		.replace(/=+$/, '');
}

export function decodeBase64url(text: string): string {
	return atob(text.replaceAll('-', '+').replaceAll('_', '/'));
}

/** JWT の中身 (claims)。署名は確かめない。読めなければ `undefined`。 */
export function jwtClaims<T>(jwt: string | undefined): T | undefined {
	try {
		return JSON.parse(decodeBase64url(jwt?.split('.')[1] ?? '')) as T;
	} catch {
		return undefined;
	}
}

export function base64urlBytes(text: string): Uint8Array {
	return Uint8Array.from(decodeBase64url(text), (c) => c.charCodeAt(0));
}

/** 人が開く画面 (サインイン・結ぶ・申し込む・移す) を置くパス。ほかは紹介のページ (site/) が使う。 */
export const ACCOUNT = '/account';
/** 公開の料金ページ (site/src/pages/pricing.astro)。 */
export const PRICING_PATH = '/pricing/';
/** アカウントのページ。 */
export const ACCOUNT_HOME = `${ACCOUNT}/`;
/** Pro を別のアカウントへ移す画面。 */
export const TRANSFER_PATH = `${ACCOUNT}/transfer`;

/**
 * サインインの後に戻る先。よそのサイトへ送られないよう、このサイトの中のパスだけを通す。
 * 空白や制御文字も通さない。ブラウザは URL のタブや改行を読み捨てるので、`/\t/evil.test` が `//evil.test` になる。
 */
export function safeNext(next: string | undefined): string {
	return next && /^\/(?![/\\])[\x21-\x7e]*$/.test(next) ? next : ACCOUNT_HOME;
}
