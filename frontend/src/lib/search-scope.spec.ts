import { describe, expect, it } from 'vitest';
import { searchScopeLevels } from './search-scope';

describe('searchScopeLevels', () => {
	it('is empty outside folder levels', () => {
		expect(searchScopeLevels(undefined)).toEqual({ upper: '', last: '' });
		expect(searchScopeLevels('')).toEqual({ upper: '', last: '' });
	});

	it('shows up to the last two levels', () => {
		expect(searchScopeLevels('2025')).toEqual({ upper: '', last: ' / 2025' });
		expect(searchScopeLevels('2025/2')).toEqual({ upper: ' / 2025', last: ' / 2' });
	});

	it('elides the levels above the last two', () => {
		expect(searchScopeLevels('eiken/2025/2')).toEqual({ upper: ' / … / 2025', last: ' / 2' });
	});
});
