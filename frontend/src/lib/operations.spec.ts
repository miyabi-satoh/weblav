import { describe, expect, it } from 'vitest';
import { opFieldLines, opPlaces, opResultLabel } from './operations';
import type { components } from '$lib/api/schema';

type Operation = components['schemas']['OperationResponse'];

const base: Operation = {
	id: 1,
	at: '2026-10-31T00:00:00Z',
	category: 'shares',
	op: 'move',
	paths: ['教材/a.pdf', '教材/b.pdf'],
	result: 'succeeded',
	shareId: 1,
	shareName: '教材',
	username: 'taro'
};

const edit: Operation = {
	...base,
	category: 'contents',
	op: 'update_content',
	paths: [],
	shareId: null,
	shareName: null,
	targetId: 7,
	targetName: '時間割'
};

describe('opPlaces', () => {
	it('joins every path and names the destination', () => {
		expect(opPlaces({ ...base, destShareName: '控え', destPath: '' })).toEqual({
			// テストの表示言語は既定の英語。
			from: '教材 / 教材 / a.pdf, 教材 / 教材 / b.pdf',
			to: '控え'
		});
	});

	it('omits the destination when there is none', () => {
		expect(opPlaces({ ...base, op: 'delete', paths: [''] })).toEqual({ from: '教材' });
	});

	it('names the target outside shares, with the new name after a rename', () => {
		expect(opPlaces(edit)).toEqual({ from: '時間割' });
		expect(opPlaces({ ...edit, op: 'rename_user', targetRenamedTo: 'hanako' })).toEqual({
			from: '時間割',
			to: 'hanako'
		});
		expect(opPlaces({ ...edit, targetPart: '学年' })).toEqual({ from: '“学年” in 時間割' });
	});

	it('has nothing to show for an operation without a target', () => {
		expect(opPlaces({ ...edit, op: 'login', targetId: null, targetName: null })).toBeUndefined();
	});
});

describe('opFieldLines', () => {
	it('shows values before and after, then the names of fields kept without values', () => {
		expect(
			opFieldLines([
				{ name: 'description' },
				{ name: 'visibility', values: { before: 'public', after: 'private' } },
				{ name: 'userAccess', subject: 'hanako', values: { before: null, after: 'all' } },
				{ name: 'somethingNew' }
			])
		).toEqual([
			// テストの表示言語は既定の英語。
			'Visibility: Everyone → Creator only',
			'Permission for hanako: No exception → Full access',
			'Changed: Description, somethingNew'
		]);
	});
});

describe('opResultLabel', () => {
	it('marks only operations that did not finish', () => {
		expect(opResultLabel(base)).toBeUndefined();
		expect(opResultLabel({ ...base, result: 'failed' })).toBeDefined();
	});
});
