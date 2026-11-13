import { describe, expect, it, vi } from 'vitest';
import { contentViewHref } from './content-view-href';

// 本物と同じく、ルートの ID (`/` で始まる) は引数を埋め、パスは頭に `/` を付ける。
vi.mock('$app/paths', () => ({
	resolve: (path: string, params?: { id: string }) =>
		path.startsWith('/') ? (params ? path.replace('[id]', params.id) : path) : `/${path}`
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
	it('opens a shortcut to a file like a file', () => {
		const shortcut = {
			...folder,
			type: 'shortcut',
			fileName: 'a.links.toml',
			visibility: 'hidden'
		} as const;
		expect(contentViewHref(shortcut, me).href).toBe('/links/10');
		expect(contentViewHref({ ...shortcut, fileName: 'a.pdf' }, me).href).toBe('/');
	});

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
