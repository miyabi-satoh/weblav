import { describe, expect, it, vi } from 'vitest';
import {
	formatByteSize,
	formatDate,
	formatDateTime,
	formatDuration,
	formatNumber,
	formatTimeAgo
} from './format';

// 表示言語は画面のトグルで変わる (→ docs/ui.md「UI 全般」)。テストからも切り替えられるようにする。
const { locale } = vi.hoisted(() => ({ locale: { current: 'ja' } }));
vi.mock('$lib/paraglide/runtime', () => ({ getLocale: () => locale.current }));

/** 端末のタイムゾーンで解釈させる (UTC 指定だと実行環境によって日付がずれる)。 */
const dateTime = new Date(2026, 8, 16, 10, 47).getTime();

describe('formatDateTime', () => {
	it('follows the display language, not the browser language', () => {
		locale.current = 'ja';
		expect(formatDateTime(dateTime)).toMatch(/^2026\/9\/16/);

		locale.current = 'en';
		expect(formatDateTime(dateTime)).toMatch(/^9\/16\/2026/);
	});

	it('returns an empty string for a missing value', () => {
		expect(formatDateTime(null)).toBe('');
		expect(formatDateTime(undefined)).toBe('');
	});
});

describe('formatDate', () => {
	it('follows the display language, not the browser language', () => {
		locale.current = 'ja';
		expect(formatDate(dateTime)).toBe('2026/9/16');

		locale.current = 'en';
		expect(formatDate(dateTime)).toBe('9/16/2026');
	});

	it('returns an empty string for a missing value', () => {
		expect(formatDate(null)).toBe('');
		expect(formatDate(undefined)).toBe('');
	});
});

describe('formatByteSize', () => {
	it('keeps bytes as-is and steps up the unit every 1024', () => {
		expect(formatByteSize(512)).toBe('512\u00a0B');
		expect(formatByteSize(1024)).toBe('1.0\u00a0KB');
		expect(formatByteSize(1024 ** 3)).toBe('1.0\u00a0GB');
	});

	it('stops at TB for anything larger', () => {
		expect(formatByteSize(1024 ** 5)).toBe('1024.0\u00a0TB');
	});

	it('returns an empty string for a missing value', () => {
		expect(formatByteSize(null)).toBe('');
		expect(formatByteSize(undefined)).toBe('');
	});
});

describe('formatNumber', () => {
	it('follows the display language, not the browser language', () => {
		locale.current = 'ja';
		expect(formatNumber(1234567)).toBe('1,234,567');

		// ja と en は桁区切りが同じなので、渡した言語が使われていること自体は別の言語で見る
		// (画面が持つのは ja / en の2つだけ)。
		locale.current = 'de';
		expect(formatNumber(1234567)).toBe('1.234.567');
	});

	it('returns an empty string for a missing value', () => {
		expect(formatNumber(null)).toBe('');
		expect(formatNumber(undefined)).toBe('');
	});
});

describe('formatDuration', () => {
	it('writes minutes and seconds, adding hours only when needed', () => {
		expect(formatDuration(0)).toBe('0:00');
		expect(formatDuration(83.9)).toBe('1:23');
		expect(formatDuration(3725)).toBe('1:02:05');
	});

	it('treats an unknown length as zero', () => {
		expect(formatDuration(Number.NaN)).toBe('0:00');
		expect(formatDuration(Number.POSITIVE_INFINITY)).toBe('0:00');
	});
});

describe('formatTimeAgo', () => {
	const now = Date.parse('2026-10-05T12:00:00Z');

	it('uses the largest unit that fits, in the display language', () => {
		locale.current = 'ja';
		expect(formatTimeAgo('2026-10-05T09:00:00Z', now)).toBe('3 時間前');
		expect(formatTimeAgo('2026-10-03T12:00:00Z', now)).toBe('一昨日');

		locale.current = 'en';
		expect(formatTimeAgo('2026-10-05T11:55:00+00:00', now)).toBe('5 minutes ago');
	});

	it('treats future and sub-minute times as just now', () => {
		locale.current = 'en';
		expect(formatTimeAgo('2026-10-05T12:00:30Z', now)).toBe('now');
		expect(formatTimeAgo('2026-10-05T11:59:30Z', now)).toBe('now');
	});

	it('returns an empty string for missing or unreadable dates', () => {
		expect(formatTimeAgo(null, now)).toBe('');
		expect(formatTimeAgo('yesterday', now)).toBe('');
	});
});
