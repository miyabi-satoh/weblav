import { describe, expect, it } from 'vitest';
import { hasViewAsCookie } from './view-as.svelte';

describe('hasViewAsCookie', () => {
	it('finds the cookie among others', () => {
		expect(hasViewAsCookie('WEBLAV_LOCALE=ja; WEBLAV_VIEW_AS=anonymous')).toBe(true);
	});

	it('ignores other values and names that only end with it', () => {
		expect(hasViewAsCookie('WEBLAV_VIEW_AS=')).toBe(false);
		expect(hasViewAsCookie('X_WEBLAV_VIEW_AS=anonymous')).toBe(false);
		expect(hasViewAsCookie('')).toBe(false);
	});
});
