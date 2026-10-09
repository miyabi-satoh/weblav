import { describe, expect, it } from 'vitest';
import { searchScopeLevel } from './search-scope';

describe('searchScopeLevel', () => {
	it('is empty outside folder levels', () => {
		expect(searchScopeLevel(undefined)).toBe('');
		expect(searchScopeLevel('')).toBe('');
	});

	it('shows up to the last two levels', () => {
		expect(searchScopeLevel('2025')).toBe(' / 2025');
		expect(searchScopeLevel('2025/2')).toBe(' / 2025 / 2');
	});

	it('elides the levels above the last two', () => {
		expect(searchScopeLevel('eiken/2025/2')).toBe(' / … / 2025 / 2');
	});
});
