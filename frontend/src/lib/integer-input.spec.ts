import { describe, expect, it } from 'vitest';
import { parseIntegerInput } from './integer-input';

describe('parseIntegerInput', () => {
	it('accepts the bounds and trims surrounding spaces', () => {
		expect(parseIntegerInput('1', 1, 10)).toEqual({ kind: 'ok', value: 1 });
		expect(parseIntegerInput(' 10 ', 1, 10)).toEqual({ kind: 'ok', value: 10 });
	});

	it('accepts full-width digits typed with the Japanese IME on', () => {
		expect(parseIntegerInput('１０', 1, 10)).toEqual({ kind: 'ok', value: 10 });
	});

	it('reports blank text as empty so that `required` can handle it', () => {
		expect(parseIntegerInput('  ', 1, 10)).toEqual({ kind: 'empty' });
	});

	it('rejects values outside the range', () => {
		expect(parseIntegerInput('0', 1, 10)).toEqual({ kind: 'invalid' });
		expect(parseIntegerInput('11', 1, 10)).toEqual({ kind: 'invalid' });
	});

	it('rejects anything but digits', () => {
		for (const text of ['-1', '1.5', '1e2', 'abc']) {
			expect(parseIntegerInput(text, 1, 10)).toEqual({ kind: 'invalid' });
		}
	});
});
