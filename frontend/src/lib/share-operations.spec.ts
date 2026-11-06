import { describe, expect, it } from 'vitest';
import { shareOpPlaces, shareOpResultLabel } from './share-operations';
import type { components } from '$lib/api/schema';

type ShareOperation = components['schemas']['ShareOperationResponse'];

const base: ShareOperation = {
	id: 1,
	at: '2026-10-31T00:00:00Z',
	op: 'move',
	paths: ['教材/a.pdf', '教材/b.pdf'],
	result: 'succeeded',
	shareId: 1,
	shareName: '教材',
	username: 'taro'
};

describe('shareOpPlaces', () => {
	it('joins every path and names the destination', () => {
		expect(shareOpPlaces({ ...base, destShareName: '控え', destPath: '' })).toEqual({
			// テストの表示言語は既定の英語。
			from: '教材 / 教材 / a.pdf, 教材 / 教材 / b.pdf',
			to: '控え'
		});
	});

	it('omits the destination when there is none', () => {
		expect(shareOpPlaces({ ...base, op: 'delete', paths: [''] })).toEqual({ from: '教材' });
	});
});

describe('shareOpResultLabel', () => {
	it('marks only operations that did not finish', () => {
		expect(shareOpResultLabel(base)).toBeUndefined();
		expect(shareOpResultLabel({ ...base, result: 'failed' })).toBeDefined();
	});
});
