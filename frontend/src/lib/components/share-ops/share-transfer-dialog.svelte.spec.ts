import { afterEach, describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { components } from '#lib/api/schema.js';
import ShareTransferDialog from './share-transfer-dialog.svelte';

const { get, post } = vi.hoisted(() => ({ get: vi.fn(), post: vi.fn() }));
vi.mock('#lib/api/client.ts', () => ({ client: { GET: get, POST: post } }));

type ShareAccess = components['schemas']['ShareAccess'];

function json(data: unknown) {
	return { data, response: new Response(null, { status: 200 }) };
}

/** 「控え」へのコピーで同じ名前にぶつかるまで進める。 */
async function openConflict(access: ShareAccess) {
	get.mockResolvedValue(
		json({
			share: { id: 2, name: '教材' },
			path: '控え',
			parent: '',
			access,
			entries: []
		})
	);
	post.mockResolvedValue(
		json({
			conflicts: [{ name: '英検', isDir: true }],
			totalBytes: 1,
			availableBytes: 2,
			trash: false,
			listed: [{ contentId: 7, title: '英検の音声' }]
		})
	);
	await render(ShareTransferDialog, {
		props: {
			open: true,
			op: 'copy',
			shareId: 2,
			dirPath: '控え',
			target: { name: '英検', path: '新/英検', isDir: true, listed: false },
			shares: [{ id: 2, name: '教材', access } as components['schemas']['ShareResponse']],
			ondone: () => {}
		}
	});
	await page.getByRole('button', { name: 'ここにコピーする' }).click();
	await expect.element(page.getByText('「控え」には「英検」がすでにあります')).toBeVisible();
}

describe('ShareTransferDialog', () => {
	afterEach(() => {
		get.mockReset();
		post.mockReset();
	});

	it('warns about contents listed on Home before overwriting', async () => {
		await openConflict('all');
		await expect.element(page.getByText('元に戻せません', { exact: false })).toBeVisible();
		await expect.element(page.getByText('英検の音声')).toBeVisible();
		await expect.element(page.getByRole('button', { name: '上書きする' })).toBeVisible();
	});

	it('does not talk about overwriting to someone who cannot overwrite', async () => {
		await openConflict('add');
		await expect.element(page.getByText('英検の音声')).not.toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: '上書きする' })).not.toBeInTheDocument();
	});

	it('does not run the transfer when the destination changes while checking', async () => {
		get.mockResolvedValue(
			json({
				share: { id: 2, name: '教材' },
				path: '控え',
				parent: '',
				access: 'all',
				entries: [{ name: '下', path: '控え/下' }]
			})
		);
		let finishCheck: (value: unknown) => void = () => {};
		post.mockReturnValueOnce(new Promise((resolve) => (finishCheck = resolve)));
		await render(ShareTransferDialog, {
			props: {
				open: true,
				op: 'copy',
				shareId: 2,
				dirPath: '控え',
				target: { name: '英検', path: '新/英検', isDir: true, listed: false },
				shares: [{ id: 2, name: '教材', access: 'all' } as components['schemas']['ShareResponse']],
				ondone: () => {}
			}
		});
		await page.getByRole('button', { name: 'ここにコピーする' }).click();
		await expect.element(page.getByRole('button', { name: 'キャンセル' })).toBeDisabled();

		await page.getByRole('button', { name: '下' }).click();
		finishCheck(
			json({ conflicts: [], totalBytes: 1, availableBytes: 2, trash: false, listed: [] })
		);

		await expect.element(page.getByRole('button', { name: 'キャンセル' })).toBeEnabled();
		expect(post).toHaveBeenCalledTimes(1);
	});
	it('stays in the folder being browsed when the share list is refreshed', async () => {
		get.mockImplementation((_url: string, options: { params: { query: { path: string } } }) => {
			const path = options.params.query.path;
			return Promise.resolve(
				json({
					share: { id: 2, name: '教材' },
					path,
					parent: path === '控え' ? '' : '控え',
					access: 'all',
					entries: path === '控え' ? [{ name: '下', path: '控え/下' }] : []
				})
			);
		});
		const share = { id: 2, name: '教材', access: 'all' } as components['schemas']['ShareResponse'];
		const props = {
			open: true,
			op: 'copy' as const,
			shareId: 2,
			dirPath: '控え',
			target: { name: '英検', path: '新/英検', isDir: true, listed: false },
			shares: [share],
			ondone: () => {}
		};
		const { rerender } = await render(ShareTransferDialog, { props });
		await page.getByRole('button', { name: '下' }).click();
		await expect.poll(() => get.mock.calls.length).toBe(2);

		// 裏の処理が終わって画面を取り直すと、同じ中身の新しい一覧が渡ってくる。
		await rerender({ shares: [{ ...share }] });

		await expect
			.element(page.getByText('このフォルダーにはサブフォルダーがありません。'))
			.toBeVisible();
		expect(get).toHaveBeenCalledTimes(2);
	});
});
