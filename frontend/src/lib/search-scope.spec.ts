import { describe, expect, it } from 'vitest';
import { searchScopeLabel } from './search-scope';

describe('searchScopeLabel', () => {
	it('uses the title alone outside folder levels', () => {
		expect(searchScopeLabel('教材', undefined)).toBe('教材');
	});

	it('puts the folder title before the level', () => {
		expect(searchScopeLabel('英検', '2025')).toBe('英検 / 2025');
	});

	it('elides the middle of deeper levels', () => {
		expect(searchScopeLabel('英検', '2025/2/grade_1')).toBe('英検 / … / grade_1');
	});
});
