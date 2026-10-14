import { env } from 'cloudflare:workers';
import { describe, expect, it } from 'vitest';
import { pricing } from '../src/pricing';

describe('pricing', () => {
	it('reads the values', () => {
		const values = {
			PURCHASE_GRANT_MILLI_YEN: '1000',
			FREE_GRANT_MILLI_YEN: '200',
			USD_JPY: '120.5',
			INPUT_USD_PER_MTOK: '0.5',
			OUTPUT_USD_PER_MTOK: '2',
			SEARCH_USD_PER_REQUEST: '0.001'
		};
		expect(pricing({ ...env, ...values } as Env)).toEqual({
			purchaseGrant: 1000,
			freeGrant: 200,
			rates: {
				usdJpy: 120.5,
				inputUsdPerMtok: 0.5,
				outputUsdPerMtok: 2,
				searchUsdPerRequest: 0.001
			}
		});
	});

	it('refuses a missing or unusable value, naming it', () => {
		for (const [name, value] of [
			['USD_JPY', undefined],
			['USD_JPY', ''],
			['USD_JPY', 'abc'],
			['USD_JPY', '0'],
			['INPUT_USD_PER_MTOK', '-1'],
			['PURCHASE_GRANT_MILLI_YEN', '1.5'],
			['FREE_GRANT_MILLI_YEN', 'Infinity'],
			['OUTPUT_USD_PER_MTOK', 'x']
		]) {
			expect(() => pricing({ ...env, [name!]: value } as Env), `${name}=${value}`).toThrow(name!);
		}
	});
});
