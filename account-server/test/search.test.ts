import { describe, expect, it } from 'vitest';
import { parseSearch, searchCost } from '../src/search';

describe('parseSearch', () => {
	it('reads the queries with the optional country, language and date filter', () => {
		expect(parseSearch({ queries: ['a', 'b'] })).toEqual({ queries: ['a', 'b'] });
		expect(parseSearch({ queries: ['a'], country: 'JP', language: 'ja' })).toEqual({
			queries: ['a'],
			country: 'JP',
			language: 'ja'
		});
		expect(parseSearch({ queries: ['a'], after: '2026-10-11' })).toEqual({
			queries: ['a'],
			after: '2026-10-11'
		});
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
			{ queries: ['a'], after: '10/11/2026' },
			{ queries: ['a'], after: '2026-02-30' }
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
