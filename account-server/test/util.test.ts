import { describe, expect, it } from 'vitest';
import { isEmail, safeNext } from '../src/util';

describe('safeNext', () => {
	it('only allows paths on this site', () => {
		expect(safeNext('/account/link?r=ABC')).toBe('/account/link?r=ABC');
		expect(safeNext('//evil.test')).toBe('/account/');
		expect(safeNext('https://evil.test')).toBe('/account/');
		expect(safeNext('/\\evil.test')).toBe('/account/');
		expect(safeNext('/\t/evil.test')).toBe('/account/');
		expect(safeNext('/\n/evil.test')).toBe('/account/');
		expect(safeNext('/ /evil.test')).toBe('/account/');
		expect(safeNext(undefined)).toBe('/account/');
	});
});

describe('isEmail', () => {
	it('accepts an address with a dotted domain and rejects the rest', () => {
		expect(isEmail('to@example.com')).toBe(true);
		for (const email of ['nope', 'typo@gmailcom', 'two@@example.com', 'a b@example.com', '']) {
			expect(isEmail(email), email).toBe(false);
		}
	});
});
