import { describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import ShareNameDialog from './share-name-dialog.svelte';

describe('ShareNameDialog', () => {
	it('selects the name without its extension when renaming a file', async () => {
		await render(ShareNameDialog, {
			props: {
				open: true,
				title: '名前の変更',
				submitLabel: '変更する',
				initialName: 'report.final.pdf',
				selectStem: true,
				request: vi.fn(),
				ondone: () => {}
			}
		});

		const input = page.getByRole('textbox', { name: '名前' });
		await expect.element(input).toHaveFocus();
		const element = input.element() as HTMLInputElement;
		expect([element.selectionStart, element.selectionEnd]).toEqual([0, 'report.final'.length]);
	});

	it('keeps the dialog open and shows why when the name is taken', async () => {
		const request = vi.fn().mockResolvedValue({
			error: {
				error: {
					code: 'invalid_request_body',
					message: '',
					detail: { kind: 'entryNameTaken', name: 'a' }
				}
			},
			response: new Response(null, { status: 422 })
		});
		const ondone = vi.fn();
		await render(ShareNameDialog, {
			props: { open: true, title: '新しいフォルダー', submitLabel: '作成する', request, ondone }
		});

		await userEvent.fill(page.getByRole('textbox', { name: '名前' }), ' a ');
		await userEvent.click(page.getByRole('button', { name: '作成する' }));

		await expect
			.element(page.getByText('「a」という名前のものがすでにあります', { exact: false }))
			.toBeVisible();
		expect(request).toHaveBeenCalledWith('a');
		expect(ondone).not.toHaveBeenCalled();
	});
});
