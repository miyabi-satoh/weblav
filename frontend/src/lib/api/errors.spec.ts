import { describe, expect, it } from 'vitest';
import { GENERIC_ERROR_MESSAGE, MESSAGES, errorCode, errorMessage } from './errors';

describe('errorCode', () => {
	it('extracts a known code from the envelope', () => {
		expect(errorCode({ error: { code: 'invalid_credentials', message: 'x' } })).toBe(
			'invalid_credentials'
		);
	});

	it('returns undefined for unknown codes and malformed bodies', () => {
		expect(errorCode({ error: { code: 'something_else' } })).toBeUndefined();
		expect(errorCode({ message: 'legacy shape' })).toBeUndefined();
		expect(errorCode('plain text')).toBeUndefined();
		expect(errorCode(undefined)).toBeUndefined();
		expect(errorCode(null)).toBeUndefined();
	});
});

describe('errorMessage', () => {
	it('maps every known code to a non-empty, code-specific message', () => {
		// 文言そのもの(Paraglideのロケール依存)ではなく、「codeごとに異なる文言を引ける」ことだけ
		// 確認する。実際の文言は messages/{locale}.json 側の責務。
		const codes = Object.keys(MESSAGES);
		const messages = new Set(codes.map((code) => errorMessage({ error: { code, message: 'x' } })));
		expect(messages.size).toBe(codes.length);
		for (const message of messages) {
			expect(message.length).toBeGreaterThan(0);
		}
	});

	it('builds a message for each Free limit target, including the limit', () => {
		const targets = ['admin', 'user', 'archive', 'folder', 'file', 'link'];
		const messages = new Set(
			targets.map((target) =>
				errorMessage({
					error: {
						code: 'free_limit_reached',
						message: 'x',
						detail: { kind: 'freeLimit', target, limit: 7 }
					}
				})
			)
		);
		expect(messages.size).toBe(targets.length);
		for (const message of messages) {
			expect(message).toContain('7');
		}
	});

	it('falls back to the generic message for malformed bodies', () => {
		expect(errorMessage(undefined)).toBe(GENERIC_ERROR_MESSAGE());
	});
});
