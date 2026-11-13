import { afterEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import ShareDeleteDialog from './share-delete-dialog.svelte';

const { post, track } = vi.hoisted(() => ({ post: vi.fn(), track: vi.fn() }));
vi.mock('#lib/api/client.ts', () => ({ client: { POST: post } }));
vi.mock('#lib/share-jobs.svelte.ts', async (original) => ({
	...(await original<typeof import('#lib/share-jobs.svelte.ts')>()),
	shareJobs: { track }
}));

const target = { name: '英検', path: '教材/英検', isDir: true, listed: true };

function json(data: unknown, status = 200) {
	return { data, response: new Response(null, { status }) };
}

describe('ShareDeleteDialog', () => {
	afterEach(() => {
		post.mockReset();
		track.mockReset();
	});

	it('warns that deleting cannot be undone where there is no trash and lists contents that stop opening', async () => {
		post.mockResolvedValueOnce(
			json({ trash: false, listed: [{ contentId: 7, title: '英検の音声' }] })
		);
		post.mockResolvedValueOnce(json({ jobId: 'j1' }, 202));
		const ondone = vi.fn();
		await render(ShareDeleteDialog, { props: { target, shareId: 2, ondone } });

		await expect.element(page.getByText('元に戻せません', { exact: false })).toBeVisible();
		await expect.element(page.getByText('英検の音声')).toBeVisible();

		await userEvent.click(page.getByRole('button', { name: '削除する' }));
		await expect.poll(() => track.mock.calls.length).toBe(1);
		expect(post).toHaveBeenLastCalledWith('/api/v1/shares/{id}/delete', {
			params: { path: { id: 2 } },
			body: { paths: ['教材/英検'], permanent: true }
		});
		expect(track.mock.calls[0][0]).toBe('j1');
		expect(track.mock.calls[0][3]).toBe(ondone);
	});

	it('sends to the trash where there is one', async () => {
		post.mockResolvedValueOnce(json({ trash: true, listed: [] }));
		await render(ShareDeleteDialog, { props: { target, shareId: 2, ondone: () => {} } });

		await expect.element(page.getByText('ごみ箱へ移します', { exact: false })).toBeVisible();
		await expect
			.element(page.getByText('元に戻せません', { exact: false }))
			.not.toBeInTheDocument();
	});
});
