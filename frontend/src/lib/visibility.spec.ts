import { describe, expect, it } from 'vitest';
import {
	effectiveVisibility,
	effectiveVisibilityUnder,
	childrenLoosenedByDeletingGroup,
	loosenedByReparent,
	selectableVisibilities
} from './visibility';

describe('effectiveVisibility', () => {
	// docs/access.md「親が外れるときの公開範囲」
	it('takes the strictest of its own visibility and the ancestors', () => {
		expect(effectiveVisibility('public', [])).toBe('public');
		expect(effectiveVisibility('public', ['public', 'authenticated'])).toBe('authenticated');
		expect(effectiveVisibility('authenticated', ['hidden', 'public'])).toBe('hidden');
		expect(effectiveVisibility('hidden', ['authenticated'])).toBe('hidden');
	});

	it('keeps private even under a hidden group', () => {
		expect(effectiveVisibility('private', ['authenticated'])).toBe('private');
		expect(effectiveVisibility('private', ['hidden'])).toBe('private');
	});
});

describe('effectiveVisibilityUnder', () => {
	it('walks every ancestor, not only the parent', () => {
		const byId = new Map([
			[1, { id: 1, parentId: null, visibility: 'authenticated' as const }],
			[2, { id: 2, parentId: 1, visibility: 'public' as const }]
		]);
		expect(effectiveVisibilityUnder('public', 2, byId)).toBe('authenticated');
		expect(effectiveVisibilityUnder('public', null, byId)).toBe('public');
	});
});

describe('childrenLoosenedByDeletingGroup', () => {
	// docs/access.md「親が外れるときの公開範囲」
	const contents = [
		{ id: 1, parentId: null, visibility: 'authenticated' as const },
		{ id: 2, parentId: 1, visibility: 'public' as const },
		{ id: 3, parentId: 1, visibility: 'hidden' as const },
		{ id: 4, parentId: 1, visibility: 'private' as const },
		{ id: 5, parentId: 2, visibility: 'public' as const },
		{ id: 6, parentId: null, visibility: 'public' as const },
		{ id: 7, parentId: 6, visibility: 'public' as const }
	];
	const byId = new Map(contents.map((content) => [content.id, content]));

	it('lists only direct children that become visible to more people', () => {
		expect(childrenLoosenedByDeletingGroup(1, contents, byId)).toEqual([
			{ content: contents[1], before: 'authenticated' }
		]);
	});

	it('counts the ancestors of the group, since children move to the root', () => {
		expect(childrenLoosenedByDeletingGroup(2, contents, byId)).toEqual([
			{ content: contents[4], before: 'authenticated' }
		]);
	});

	it('lists nothing when the group does not narrow its children', () => {
		expect(childrenLoosenedByDeletingGroup(6, contents, byId)).toEqual([]);
	});
});

describe('loosenedByReparent', () => {
	// docs/access.md「親が外れるときの公開範囲」
	const byId = new Map([
		[1, { id: 1, parentId: null, visibility: 'authenticated' as const }],
		[2, { id: 2, parentId: null, visibility: 'public' as const }],
		[3, { id: 3, parentId: 1, visibility: 'public' as const }]
	]);

	it('reports the views before and after when moving out of a stricter group', () => {
		expect(
			loosenedByReparent(
				{ visibility: 'public', parentId: 1 },
				{ visibility: 'public', parentId: 2 },
				byId
			)
		).toEqual({ before: 'authenticated', after: 'public' });
		expect(
			loosenedByReparent(
				{ visibility: 'public', parentId: 3 },
				{ visibility: 'public', parentId: null },
				byId
			)
		).toEqual({ before: 'authenticated', after: 'public' });
	});

	it('reports nothing when the parent stays the same', () => {
		// 公開範囲だけを緩めるのは、操作した人が選んだ結果そのもの。
		expect(
			loosenedByReparent(
				{ visibility: 'authenticated', parentId: 2 },
				{ visibility: 'public', parentId: 2 },
				byId
			)
		).toBeNull();
		expect(
			loosenedByReparent(
				{ visibility: 'public', parentId: undefined },
				{ visibility: 'public', parentId: null },
				byId
			)
		).toBeNull();
	});

	it('reports nothing when the move does not widen the view', () => {
		expect(
			loosenedByReparent(
				{ visibility: 'public', parentId: 2 },
				{ visibility: 'public', parentId: 1 },
				byId
			)
		).toBeNull();
		expect(
			loosenedByReparent(
				{ visibility: 'public', parentId: 1 },
				{ visibility: 'public', parentId: 3 },
				byId
			)
		).toBeNull();
		expect(
			loosenedByReparent(
				{ visibility: 'private', parentId: 1 },
				{ visibility: 'private', parentId: null },
				byId
			)
		).toBeNull();
	});
});

describe('selectableVisibilities', () => {
	const me = 1;
	const other = 2;

	it('offers private when creating a non-group content', () => {
		expect(selectableVisibilities('link', null, me)).toEqual([
			'public',
			'authenticated',
			'private',
			'hidden'
		]);
	});

	it('never offers private for a group', () => {
		expect(selectableVisibilities('group', null, me)).toEqual([
			'public',
			'authenticated',
			'hidden'
		]);
		expect(selectableVisibilities('group', { visibility: 'public', createdBy: me }, me)).toEqual([
			'public',
			'authenticated',
			'hidden'
		]);
	});

	it('offers private when editing content the user created', () => {
		expect(selectableVisibilities('file', { visibility: 'public', createdBy: me }, me)).toEqual([
			'public',
			'authenticated',
			'private',
			'hidden'
		]);
		expect(selectableVisibilities('file', { visibility: 'private', createdBy: me }, me)).toEqual([
			'public',
			'authenticated',
			'private',
			'hidden'
		]);
	});

	it('does not offer private when editing content someone else created', () => {
		expect(selectableVisibilities('link', { visibility: 'public', createdBy: other }, me)).toEqual([
			'public',
			'authenticated',
			'hidden'
		]);
	});

	it('does not offer private when editing content without a creator', () => {
		expect(selectableVisibilities('link', { visibility: 'public', createdBy: null }, me)).toEqual([
			'public',
			'authenticated',
			'hidden'
		]);
	});

	it('offers only private and hidden when editing private content someone else created', () => {
		expect(selectableVisibilities('link', { visibility: 'private', createdBy: other }, me)).toEqual(
			['private', 'hidden']
		);
	});
});
