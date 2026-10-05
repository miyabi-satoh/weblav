import { describe, expect, it } from 'vitest';
import { LatestRequest } from './latest-request';

describe('LatestRequest', () => {
	it('marks an earlier request as stale once a newer one begins', () => {
		const requests = new LatestRequest();
		const first = requests.begin();
		const second = requests.begin();

		expect(first()).toBe(false);
		expect(second()).toBe(true);
	});

	it('marks the in-flight request as stale on invalidate', () => {
		const requests = new LatestRequest();
		const current = requests.begin();
		requests.invalidate();

		expect(current()).toBe(false);
	});
});
