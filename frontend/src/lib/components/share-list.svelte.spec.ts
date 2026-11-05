import { describe, expect, it } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import ShareList from './share-list.svelte';

describe('ShareList', () => {
	it('links each share to its page and tells what the user can do in it', async () => {
		await render(ShareList, {
			shares: [
				{ id: 1, name: 'Music', access: 'read' },
				{ id: 2, name: 'Uploads', access: 'add' },
				{ id: 3, name: 'Projects', access: 'all' }
			]
		});

		await expect
			.element(page.getByRole('link', { name: 'Music' }))
			.toHaveAttribute('href', '/shares/1');
		await expect.element(page.getByText('読み取り専用')).toBeVisible();
		await expect.element(page.getByText('追加のみ')).toBeVisible();
		await expect.element(page.getByText('フルアクセス')).toBeVisible();
	});
});
