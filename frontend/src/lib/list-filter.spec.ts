import { describe, expect, it } from 'vitest';
import { highlightSegments, matchFilter, parseFilterTerms } from './list-filter';

function match(text: string, query: string) {
	return matchFilter(text, parseFilterTerms(query));
}

describe('matchFilter', () => {
	it('prefers a contiguous match', () => {
		expect(match('リスニング リスト', 'リスト')).toEqual([6, 7, 8]);
	});

	it('falls back to characters in order with gaps', () => {
		expect(match('英検 リスニング 第2回', '英リス2')).toEqual([0, 3, 4, 10]);
	});

	it('rejects characters out of order', () => {
		expect(match('英検 リスニング', 'ス英')).toBeNull();
	});

	it('requires every space-separated term', () => {
		expect(match('2025 リスニング', '2025　リス')).toEqual([0, 1, 2, 3, 5, 6]);
		expect(match('2025 リスニング', '2024 リス')).toBeNull();
	});

	it('ignores width and case', () => {
		expect(match('ABC教材', 'ａｂｃ')).toEqual([0, 1, 2]);
		expect(match('バイオ', 'ﾊﾞｲｵ')).toEqual([0, 1, 2]);
		// 元の文字列が半角なら、濁点も含めて強調する。
		expect(match('ﾊﾞｲｵ', 'バイ')).toEqual([0, 1, 2]);
	});

	it('keeps everything when there are no terms', () => {
		expect(parseFilterTerms('  　')).toEqual([]);
		expect(match('なんでも', '')).toEqual([]);
	});
});

describe('highlightSegments', () => {
	it('groups neighbouring hits', () => {
		expect(highlightSegments('英検 リスニング', [0, 3, 4])).toEqual([
			{ text: '英', hit: true },
			{ text: '検 ', hit: false },
			{ text: 'リス', hit: true },
			{ text: 'ニング', hit: false }
		]);
	});
});
