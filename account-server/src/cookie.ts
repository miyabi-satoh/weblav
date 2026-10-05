import type { Context } from 'hono';
import { deleteCookie, setCookie } from 'hono/cookie';
import { isHttps } from './util';

/**
 * https では `__Host-` を付け、`Path=/`・`Domain` 無しをブラウザに守らせる。
 * amiiby.com の別のサブドメインから同名の Cookie を送り込まれないように。
 */
export function hostCookieName(c: Context, base: string): string {
	return isHttps(c) ? `__Host-${base}` : base;
}

/**
 * `SameSite` は Strict にしない。WebLAV の画面やメールのリンク・Google から戻るとき (よそのサイトからの移動) に、
 * Cookie が送られなくなるため。書き込みは Origin の確かめ (hono/csrf) で守る。
 */
export function setHostCookie(c: Context, base: string, value: string, maxAge: number) {
	setCookie(c, hostCookieName(c, base), value, {
		path: '/',
		httpOnly: true,
		secure: isHttps(c),
		sameSite: 'Lax',
		maxAge
	});
}

export function deleteHostCookie(c: Context, base: string) {
	deleteCookie(c, hostCookieName(c, base), { path: '/', secure: isHttps(c) });
}
