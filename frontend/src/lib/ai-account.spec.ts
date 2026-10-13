import { describe, expect, it, vi } from 'vitest';
import { accountPageHref, formatPercent, usagePercent } from './ai-account';

const { locale } = vi.hoisted(() => ({ locale: { current: 'ja' } }));
vi.mock('$lib/paraglide/runtime', () => ({ getLocale: () => locale.current }));

describe('formatPercent', () => {
	it('keeps at most one decimal place', () => {
		expect(formatPercent(100)).toBe('100%');
		expect(formatPercent(82.5)).toBe('82.5%');
		expect(formatPercent(33.333)).toBe('33.3%');
	});
});

describe('usagePercent', () => {
	it('converts the amount to a percentage of one purchase', () => {
		expect(usagePercent(1_000_000)).toEqual({ belowMinimum: false, text: '100%' });
		expect(usagePercent(123_400)).toEqual({ belowMinimum: false, text: '12.3%' });
		expect(usagePercent(1_000)).toEqual({ belowMinimum: false, text: '0.1%' });
	});

	it('can exceed one purchase', () => {
		expect(usagePercent(2_500_000)).toEqual({ belowMinimum: false, text: '250%' });
	});

	it('marks an amount that would round to zero as below the minimum', () => {
		expect(usagePercent(1)).toEqual({ belowMinimum: true, text: '0.1%' });
		expect(usagePercent(999)).toEqual({ belowMinimum: true, text: '0.1%' });
	});

	it('shows zero as zero', () => {
		expect(usagePercent(0)).toEqual({ belowMinimum: false, text: '0%' });
	});
});

describe('accountPageHref', () => {
	it('adds the display language to a URL without a query', () => {
		locale.current = 'ja';
		expect(accountPageHref('https://example.com/account/')).toBe(
			'https://example.com/account/?lang=ja'
		);
	});

	it('keeps the existing query', () => {
		locale.current = 'en';
		expect(accountPageHref('https://example.com/account/link?challenge=ab&name=A%26B')).toBe(
			'https://example.com/account/link?challenge=ab&name=A%26B&lang=en'
		);
		locale.current = 'ja';
	});
});
