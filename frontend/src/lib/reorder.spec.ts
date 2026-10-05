import { describe, expect, it } from 'vitest';
import { moveItem } from './reorder';

describe('moveItem', () => {
	it('前にある要素を後ろへ動かす', () => {
		expect(moveItem(['a', 'b', 'c', 'd'], 0, 2)).toEqual(['b', 'c', 'a', 'd']);
	});

	it('後ろにある要素を前へ動かす', () => {
		expect(moveItem(['a', 'b', 'c', 'd'], 3, 1)).toEqual(['a', 'd', 'b', 'c']);
	});

	it('同じ位置なら並びは変わらない', () => {
		expect(moveItem(['a', 'b', 'c'], 1, 1)).toEqual(['a', 'b', 'c']);
	});

	it('取り出す位置が範囲外なら並びを変えない', () => {
		expect(moveItem(['a', 'b'], 2, 0)).toEqual(['a', 'b']);
		expect(moveItem(['a', 'b'], -1, 0)).toEqual(['a', 'b']);
	});

	it('入れる位置は範囲内に収める', () => {
		expect(moveItem(['a', 'b', 'c'], 0, 9)).toEqual(['b', 'c', 'a']);
	});

	it('元の配列は変えない', () => {
		const items = ['a', 'b', 'c'];
		moveItem(items, 0, 2);
		expect(items).toEqual(['a', 'b', 'c']);
	});
});
