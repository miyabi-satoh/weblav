import { describe, expect, it } from 'vitest';
import { searchScopeFromQuery, searchScopeLevels } from './search-scope';

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

describe('searchScopeFromQuery', () => {
	const scopeOf = (query: string) => searchScopeFromQuery(new URL(`http://x/search?${query}`));

	it('reads a share and its level', () => {
		expect(scopeOf('share=3&path=2024')).toEqual({ share: '3', path: '2024' });
		expect(scopeOf('share=3')).toEqual({ share: '3' });
	});

	it('ignores ids that are not numbers', () => {
		expect(scopeOf('share=x')).toBeNull();
		expect(scopeOf('within=x&share=3')).toBeNull();
	});
});
