import { describe, expect, it } from 'vitest';
import {
	effectiveVisibility,
	effectiveVisibilityUnder,
	childrenLoosenedByDeletingGroup,
	loosenedByReparent,
	selectableVisibilities,
	fitsParent,
	childrenFitPrivateGroup
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
	const all = ['public', 'authenticated', 'private', 'hidden'];
	const withoutPrivate = ['public', 'authenticated', 'hidden'];

	it('offers private when creating content, groups included', () => {
		expect(selectableVisibilities(null, me, null)).toEqual(all);
	});

	it('offers private when editing content the user created', () => {
		expect(selectableVisibilities({ visibility: 'public', createdBy: me }, me, null)).toEqual(all);
		expect(selectableVisibilities({ visibility: 'private', createdBy: me }, me, null)).toEqual(all);
	});

	it('does not offer private when editing content someone else or nobody created', () => {
		expect(selectableVisibilities({ visibility: 'public', createdBy: other }, me, null)).toEqual(
			withoutPrivate
		);
		expect(selectableVisibilities({ visibility: 'public', createdBy: null }, me, null)).toEqual(
			withoutPrivate
		);
	});

	it('offers only private and hidden when editing private content someone else created', () => {
		expect(selectableVisibilities({ visibility: 'private', createdBy: other }, me, null)).toEqual([
			'private',
			'hidden'
		]);
	});

	it('offers only private inside a private group', () => {
		const parent = { visibility: 'private', createdBy: me } as const;
		expect(selectableVisibilities(null, me, parent)).toEqual(['private']);
		expect(selectableVisibilities({ visibility: 'public', createdBy: me }, me, parent)).toEqual([
			'private'
		]);
	});
});

describe('fitsParent', () => {
	const me = 1;
	const other = 2;
	const privateGroup = { visibility: 'private', createdBy: me } as const;

	it('admits only the group creator’s private content into a private group', () => {
		expect(fitsParent(privateGroup, { visibility: 'private', createdBy: me })).toBe(true);
		expect(fitsParent(privateGroup, { visibility: 'public', createdBy: me })).toBe(false);
		expect(fitsParent(privateGroup, { visibility: 'private', createdBy: other })).toBe(false);
		expect(fitsParent(privateGroup, { visibility: 'private', createdBy: null })).toBe(false);
	});

	it('admits anything into other groups and the root', () => {
		expect(fitsParent({ visibility: 'public', createdBy: me }, { visibility: 'public' })).toBe(
			true
		);
		expect(fitsParent(null, { visibility: 'public' })).toBe(true);
	});
});

describe('childrenFitPrivateGroup', () => {
	const group = { id: 10, createdBy: 1 };

	it('requires every direct child to be the creator’s private content', () => {
		const own = { id: 11, parentId: 10, visibility: 'private', createdBy: 1 } as const;
		const shared = { id: 12, parentId: 10, visibility: 'public', createdBy: 1 } as const;
		const elsewhere = { id: 13, parentId: null, visibility: 'public', createdBy: 2 } as const;
		expect(childrenFitPrivateGroup(group, [own, elsewhere])).toBe(true);
		expect(childrenFitPrivateGroup(group, [own, shared])).toBe(false);
		expect(childrenFitPrivateGroup(group, [])).toBe(true);
	});
});
