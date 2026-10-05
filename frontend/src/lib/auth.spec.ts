import { describe, expect, it } from 'vitest';
import {
	canEditContent,
	isAdmin,
	isApiTarget,
	isProtectedRoute,
	loginPathWithRedirect,
	safeRedirectTarget
} from './auth';

describe('isAdmin', () => {
	it('is true only for a logged-in admin', () => {
		expect(isAdmin({ role: 'admin' })).toBe(true);
		expect(isAdmin({ role: 'user' })).toBe(false);
		expect(isAdmin(null)).toBe(false);
		expect(isAdmin(undefined)).toBe(false);
	});
});

describe('canEditContent', () => {
	it('lets an admin edit anything and a user only their own', () => {
		const alice = { id: 2, role: 'user' as const };
		expect(canEditContent({ id: 1, role: 'admin' }, { createdBy: 2 })).toBe(true);
		expect(canEditContent({ id: 1, role: 'admin' }, { createdBy: null })).toBe(true);
		expect(canEditContent(alice, { createdBy: 2 })).toBe(true);
		expect(canEditContent(alice, { createdBy: 3 })).toBe(false);
		expect(canEditContent(alice, { createdBy: null })).toBe(false);
		expect(canEditContent(null, { createdBy: 2 })).toBe(false);
	});
});

describe('isProtectedRoute', () => {
	it('protects /admin and everything under it', () => {
		expect(isProtectedRoute('/admin')).toBe(true);
		expect(isProtectedRoute('/admin/contents')).toBe(true);
	});

	it('leaves viewing routes open to anonymous visitors', () => {
		expect(isProtectedRoute('/')).toBe(false);
		expect(isProtectedRoute('/login')).toBe(false);
		expect(isProtectedRoute('/groups/[id]')).toBe(false);
		expect(isProtectedRoute('/folders/[id]')).toBe(false);
		expect(isProtectedRoute(null)).toBe(false);
	});

	it('does not treat a route that merely starts with the same letters as protected', () => {
		expect(isProtectedRoute('/administrators')).toBe(false);
	});
});

describe('loginPathWithRedirect', () => {
	it('encodes the target so query strings survive the round trip', () => {
		// 空白は `+` になる (`withQuery` は `URLSearchParams` で符号化する)。
		const path = loginPathWithRedirect('/login', '/folders/1?path=a b');
		expect(path).toBe('/login?redirect=%2Ffolders%2F1%3Fpath%3Da+b');
		// 読み戻して同じ値になることまで見る。符号化の形は変わってもよい。
		expect(new URL(path, 'http://x').searchParams.get('redirect')).toBe('/folders/1?path=a b');
	});
});

describe('isApiTarget', () => {
	it('recognises API paths, which SvelteKit cannot resolve as routes', () => {
		expect(isApiTarget('/api/v1/contents/1/download')).toBe(true);
		expect(isApiTarget('/api/v1/contents/1/download?path=a.pdf')).toBe(true);
	});

	it('leaves in-app routes alone', () => {
		expect(isApiTarget('/folders/1')).toBe(false);
		expect(isApiTarget('/admin/contents')).toBe(false);
		// パスの途中に api があるだけのものは対象外。
		expect(isApiTarget('/groups/api')).toBe(false);
	});
});

describe('safeRedirectTarget', () => {
	it('accepts same-site absolute paths', () => {
		expect(safeRedirectTarget('/groups/1')).toBe('/groups/1');
		expect(safeRedirectTarget('/folders/1?path=sub')).toBe('/folders/1?path=sub');
	});

	it('rejects anything that could leave the site (open redirect)', () => {
		expect(safeRedirectTarget('//example.com')).toBeNull();
		expect(safeRedirectTarget('/\\example.com')).toBeNull();
		expect(safeRedirectTarget('https://example.com')).toBeNull();
		expect(safeRedirectTarget('groups/1')).toBeNull();
		expect(safeRedirectTarget(null)).toBeNull();
	});
});
