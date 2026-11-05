import { describe, expect, it, vi } from 'vitest';
import { contentViewHref } from './content-view-href';

vi.mock('$app/paths', () => ({
	resolve: (path: string, params?: { id: string }) =>
		params ? path.replace('[id]', params.id) : path
}));

const me = 1;
const other = 2;
const folder = {
	id: 10,
	type: 'folder',
	parentId: null,
	url: null,
	fileName: null,
	listView: false,
	visibility: 'public',
	createdBy: other
} as const;

describe('contentViewHref', () => {
	it('opens the list the content is in, so the result can be seen among its neighbors', () => {
		expect(contentViewHref(folder, me).href).toBe('/');
		expect(contentViewHref({ ...folder, parentId: 5 }, me).href).toBe('/groups/5');
		expect(
			contentViewHref({ ...folder, type: 'link', url: 'https://example.com', parentId: 5 }, me)
		).toEqual({ href: '/groups/5', external: false });
		expect(contentViewHref({ ...folder, visibility: 'private', createdBy: me }, me).href).toBe('/');
	});

	it('opens the content itself when the list above does not show it', () => {
		expect(contentViewHref({ ...folder, visibility: 'hidden' }, me).href).toBe('/folders/10');
		expect(contentViewHref({ ...folder, visibility: 'private' }, me).href).toBe('/folders/10');
		expect(contentViewHref({ ...folder, visibility: 'hidden', listView: true }, me).href).toBe(
			'/archives/10'
		);
		expect(contentViewHref({ ...folder, type: 'group', visibility: 'hidden' }, me).href).toBe(
			'/groups/10'
		);
		expect(
			contentViewHref(
				{ ...folder, type: 'link', url: 'https://example.com', visibility: 'hidden' },
				me
			)
		).toEqual({ href: 'https://example.com', external: true });
	});
});
