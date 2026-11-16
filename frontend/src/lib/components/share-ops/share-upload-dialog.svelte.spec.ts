import { afterEach, describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { beforeNavigate as BeforeNavigateFn } from '$app/navigation';
import ShareUploadDialog from './share-upload-dialog.svelte';

type BeforeNavigate = Parameters<Parameters<typeof BeforeNavigateFn>[0]>[0];

const { beforeNavigate, goto, uploadShareFile } = vi.hoisted(() => ({
	beforeNavigate: vi.fn(),
	goto: vi.fn(),
	uploadShareFile: vi.fn()
}));
vi.mock('$app/navigation', () => ({ beforeNavigate, goto }));
vi.mock('#lib/share-upload.ts', () => ({
	shareUploadUrl: () => '/api/v1/shares/2/upload',
	uploadShareFile
}));

async function renderDialog() {
	await render(ShareUploadDialog, {
		props: {
			open: true,
			shareId: 2,
			path: '教材',
			folderName: '教材',
			maxBytes: null,
			ondone: () => {}
		}
	});
}

/** ブラウザーが落とすときと同じ形の drop。フォルダーかは `items` の項目が答える。 */
function drop(target: Element, files: { file: File; isDirectory: boolean }[]) {
	const transfer = new DataTransfer();
	for (const { file } of files) transfer.items.add(file);
	const event = new Event('drop', { bubbles: true, cancelable: true });
	Object.defineProperty(event, 'dataTransfer', {
		value: {
			files: transfer.files,
			items: files.map(({ isDirectory }) => ({
				kind: 'file',
				webkitGetAsEntry: () => ({ isDirectory })
			}))
		}
	});
	target.dispatchEvent(event);
	return event;
}

function dropzone(): Element {
	const zone = document.querySelector('label[for="share-upload-files"]');
	if (!zone) throw new Error('ドロップの場所が無い');
	return zone;
}

describe('ShareUploadDialog', () => {
	afterEach(() => {
		beforeNavigate.mockReset();
		goto.mockReset();
		uploadShareFile.mockReset();
	});

	it('says a dropped folder cannot be uploaded instead of sending it', async () => {
		await renderDialog();
		drop(dropzone(), [
			{ file: new File([], '写真'), isDirectory: true },
			{ file: new File(['x'], 'a.txt'), isDirectory: false }
		]);

		await expect.element(page.getByText('フォルダーはアップロードできません。')).toBeVisible();
		uploadShareFile.mockResolvedValue({
			ok: true,
			file: { name: 'a.txt', renamed: false, extensionFixed: false }
		});
		await page.getByRole('button', { name: 'アップロードする' }).click();
		await expect.poll(() => uploadShareFile.mock.calls.length).toBe(1);
		expect(uploadShareFile.mock.calls[0][1].name).toBe('a.txt');
	});

	it('keeps the browser from opening a file dropped outside the drop area', async () => {
		await renderDialog();
		const event = drop(document.body, [{ file: new File(['x'], 'a.txt'), isDirectory: false }]);
		expect(event.defaultPrevented).toBe(true);
	});

	it('asks before leaving during an upload and stops the rest when leaving', async () => {
		let signal: AbortSignal | undefined;
		uploadShareFile.mockImplementation(
			(_url: string, _file: File, _progress: unknown, given: AbortSignal) =>
				new Promise((resolve) => {
					signal = given;
					given.addEventListener('abort', () => resolve({ ok: false, message: '', aborted: true }));
				})
		);
		await renderDialog();
		drop(dropzone(), [
			{ file: new File(['x'], 'a.txt'), isDirectory: false },
			{ file: new File(['y'], 'b.txt'), isDirectory: false }
		]);
		await page.getByRole('button', { name: 'アップロードする' }).click();
		await expect.poll(() => signal).toBeDefined();

		const guard: (navigation: BeforeNavigate) => void = beforeNavigate.mock.calls[0][0];
		const cancel = vi.fn();
		const to = new URL('http://localhost/shares/2');
		guard({ type: 'link', to: { url: to }, cancel } as unknown as BeforeNavigate);

		expect(cancel).toHaveBeenCalled();
		await page.getByRole('button', { name: '中止して移動する' }).click();
		await expect.poll(() => goto.mock.calls.length).toBe(1);
		expect(goto).toHaveBeenCalledWith(to);
		expect(signal?.aborted).toBe(true);
		expect(uploadShareFile).toHaveBeenCalledTimes(1);
	});
});
