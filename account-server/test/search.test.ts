import { describe, expect, it } from 'vitest';
import { parseSearch, searchCost } from '../src/search';

describe('parseSearch', () => {
	it('reads the queries with the optional country, language and recency', () => {
		expect(parseSearch({ queries: ['a', 'b'] })).toEqual({ queries: ['a', 'b'] });
		expect(parseSearch({ queries: ['a'], country: 'JP', language: 'ja', recency: 'year' })).toEqual(
			{ queries: ['a'], country: 'JP', language: 'ja', recency: 'year' }
		);
	});

	it('refuses other shapes', () => {
		for (const body of [
			undefined,
			null,
			{},
			{ queries: 'a' },
			{ queries: [] },
			{ queries: ['a', 'b', 'c', 'd', 'e', 'f'] },
			{ queries: ['  '] },
			{ queries: [1] },
			{ queries: ['x'.repeat(201)] },
			{ queries: ['a'], country: 'jp' },
			{ queries: ['a'], language: 'JA' },
			// 1か月以下の絞り込みは、量産の記事が多く混じるので受けない。
			{ queries: ['a'], recency: 'month' }
		]) {
			expect(parseSearch(body), JSON.stringify(body)).toBeUndefined();
		}
	});
});

describe('searchCost', () => {
	it('rounds one request up to milli_yen', () => {
		const rates = {
			inputUsdPerMtok: 1,
			outputUsdPerMtok: 4,
			usdJpy: 150.5,
			searchUsdPerRequest: 0.001
		};
		expect(searchCost(rates)).toBe(151);
	});
});
