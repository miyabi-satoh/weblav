import { describe, expect, it } from 'vitest';
import { isLoopbackHost } from './loopback';

describe('isLoopbackHost', () => {
	it('accepts what the server accepts as a loopback host', () => {
		expect(isLoopbackHost('localhost')).toBe(true);
		expect(isLoopbackHost('127.0.0.1')).toBe(true);
		// ループバックは 127.0.0.0/8 全体。
		expect(isLoopbackHost('127.0.0.53')).toBe(true);
		expect(isLoopbackHost('[::1]')).toBe(true);
	});

	it('rejects LAN addresses and names that merely point at loopback', () => {
		expect(isLoopbackHost('192.168.1.5')).toBe(false);
		expect(isLoopbackHost('weblav.local')).toBe(false);
		// 127.0.0.1 に向けられていても、名前では通さない (DNS リバインディングと区別が付かない)。
		expect(isLoopbackHost('evil.example')).toBe(false);
		expect(isLoopbackHost('')).toBe(false);
	});
});
