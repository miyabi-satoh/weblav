import { afterEach, describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import DirPicker from './dir-picker.svelte';

const { get } = vi.hoisted(() => ({ get: vi.fn() }));
vi.mock('#lib/api/client.ts', () => ({ client: { GET: get } }));

const SHARES = {
	path: '',
	parent: null,
	root: null,
	rootName: null,
	selectable: false,
	entries: [{ name: '教材', path: '/srv/教材', selectable: true }],
	files: []
};

const MATERIALS = {
	path: '/srv/教材',
	parent: '',
	root: '/srv/教材',
	rootName: '教材',
	selectable: true,
	entries: [{ name: '英検', path: '/srv/教材/英検', selectable: true }],
	files: []
};

function ok(data: unknown) {
	return { data, response: new Response(null, { status: 200 }) };
}

function unreadable() {
	return {
		data: undefined,
		error: { error: { code: 'not_found', message: 'not found' } },
		response: new Response(null, { status: 422 })
	};
}

function listedPath(): string | undefined {
	return get.mock.lastCall?.[1]?.params?.query?.path;
}

describe('DirPicker', () => {
	afterEach(() => {
		get.mockReset();
	});

	it('falls back to the shares list when the starting folder cannot be read', async () => {
		get.mockImplementation((_url, { params }) =>
			Promise.resolve(params.query.path === '' ? ok(SHARES) : unreadable())
		);
		await render(DirPicker, { open: true, initialPath: '/srv/消えた', onselect: () => {} });

		await expect.element(page.getByRole('button', { name: '教材' })).toBeVisible();
		expect(listedPath()).toBe('');
	});

	it('stays in the current folder and shows the reason when a clicked folder cannot be read', async () => {
		get.mockImplementation((_url, { params }) =>
			Promise.resolve(params.query.path === '/srv/教材' ? ok(MATERIALS) : unreadable())
		);
		await render(DirPicker, { open: true, initialPath: '/srv/教材', onselect: () => {} });
		await page.getByRole('button', { name: '英検' }).click();

		await expect
			.element(page.getByRole('alert'))
			.toHaveTextContent('指定されたデータが見つかりませんでした。');
		await expect.element(page.getByRole('button', { name: '英検' })).toBeVisible();
		expect(get.mock.calls.map((call) => call[1].params.query.path)).toEqual([
			'/srv/教材',
			'/srv/教材/英検'
		]);
	});
});
