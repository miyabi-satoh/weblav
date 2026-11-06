import { describe, expect, it, vi } from 'vitest';
import type { components } from '$lib/api/schema';
import { contentEditBack } from './content-edit-back';

vi.mock('$app/paths', () => ({
	resolve: (path: string, params?: { id: string }) =>
		params ? path.replace('[id]', params.id) : path
}));

type Content = components['schemas']['AdminContentResponse'];

const user = { role: 'user' } as const;
const group = { id: 5, title: '教材' } as Content;
const contentById = new Map([[group.id, group]]);

describe('contentEditBack', () => {
	it('sends an admin back to the content list', () => {
		const back = contentEditBack(
			{ role: 'admin' },
			{ parentId: 5, visibility: 'hidden' },
			contentById
		);
		expect(back.href).toBe('/admin/contents');
	});

	it('sends a user back to the group the content is listed in', () => {
		const back = contentEditBack(user, { parentId: 5, visibility: 'public' }, contentById);
		expect(back).toEqual({ href: '/groups/5', label: '教材' });
	});

	it('sends a user back to Home for content at the top level', () => {
		expect(contentEditBack(user, { parentId: null, visibility: 'public' }, contentById).href).toBe(
			'/'
		);
	});

	it('sends a user back to the hidden list for hidden content, which no group lists', () => {
		expect(contentEditBack(user, { parentId: 5, visibility: 'hidden' }, contentById).href).toBe(
			'/hidden'
		);
	});
});
