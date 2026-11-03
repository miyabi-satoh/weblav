import { afterEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import VisibilityDialog from './visibility-dialog.svelte';

const { put } = vi.hoisted(() => ({ put: vi.fn() }));
vi.mock('$lib/api/client', () => ({ client: { PUT: put } }));

describe('VisibilityDialog', () => {
	afterEach(() => {
		put.mockReset();
	});

	it('offers only the given options and starts from the current visibility', async () => {
		await render(VisibilityDialog, {
			props: {
				target: {
					id: 5,
					title: 'Lesson',
					visibility: 'authenticated',
					options: ['public', 'authenticated', 'hidden']
				},
				onsaved: () => {}
			}
		});

		await expect.element(page.getByRole('radio', { name: 'ログインした人' })).toBeChecked();
		expect(page.getByRole('radio').all()).toHaveLength(3);
		await expect.element(page.getByRole('radio', { name: '作成者のみ' })).not.toBeInTheDocument();
	});

	it('sends the chosen visibility and reports the change', async () => {
		put.mockResolvedValue({ response: new Response(null, { status: 204 }) });
		const onsaved = vi.fn();
		await render(VisibilityDialog, {
			props: {
				target: {
					id: 5,
					title: 'Lesson',
					visibility: 'public',
					options: ['public', 'authenticated', 'private', 'hidden']
				},
				onsaved
			}
		});

		await userEvent.click(page.getByRole('radio', { name: '作成者のみ' }));
		await userEvent.click(page.getByRole('button', { name: '保存する' }));

		await expect.poll(() => onsaved.mock.calls.length).toBe(1);
		expect(put).toHaveBeenCalledWith('/api/v1/contents/{id}/visibility', {
			params: { path: { id: 5 } },
			body: { visibility: 'private' }
		});
	});
});
