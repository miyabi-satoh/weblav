import { describe, expect, it, vi } from 'vitest';
import type { components } from '$lib/api/schema';
import { canAddRoots, contentPathLabel, rootLocationLabel } from './root-location';

vi.mock('$lib/paraglide/messages.js', () => ({
	contents_path_root_deleted: () => '(見つからない公開できるフォルダー)'
}));

type Content = components['schemas']['AdminContentResponse'];

function content(fields: Partial<Content>): Content {
	return {
		id: 1,
		type: 'archive',
		parentId: null,
		title: '英検',
		url: null,
		path: '/srv/教材/英検',
		rootName: null,
		pathInRoot: null,
		rootDeleted: false,
		description: null,
		fileName: null,
		fileSize: null,
		visibility: 'public',
		createdBy: null,
		createdByUsername: null,
		extensions: null,
		titleTemplate: null,
		...fields
	} as Content;
}

describe('rootLocationLabel', () => {
	it('joins the root name and the rest', () => {
		expect(rootLocationLabel('教材', '英検/2024')).toBe('教材 / 英検 / 2024');
		expect(rootLocationLabel('教材', '')).toBe('教材');
	});
});

describe('contentPathLabel', () => {
	const registered = content({ rootName: '教材', pathInRoot: '英検' });
	const deleted = content({ rootName: '教材', pathInRoot: '英検', rootDeleted: true });
	const unknown = content({});

	it('shows the root name inside a registered root, to everyone', () => {
		expect(contentPathLabel(registered, false)).toBe('教材 / 英検');
		expect(contentPathLabel(registered, true)).toBe('教材 / 英検');
	});

	it('hides the root path of a deleted root unless the viewer is an admin', () => {
		expect(contentPathLabel(deleted, false)).toBe('(見つからない公開できるフォルダー) / 英検');
		expect(contentPathLabel(deleted, true)).toBe('/srv/教材/英検');
	});

	it('shows nothing of the path outside any root record unless the viewer is an admin', () => {
		expect(contentPathLabel(unknown, false)).toBe('(見つからない公開できるフォルダー)');
		expect(contentPathLabel(unknown, true)).toBe('/srv/教材/英検');
	});

	it('has no location without a path', () => {
		expect(contentPathLabel(content({ type: 'link', path: null }), false)).toBeNull();
	});
});

describe('canAddRoots', () => {
	it('is true only on the server PC', () => {
		expect(canAddRoots('127.0.0.1')).toBe(true);
		expect(canAddRoots('192.168.1.5')).toBe(false);
	});
});
