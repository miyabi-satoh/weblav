// 共有の中の画面と、ホームの「共有」の区画 (→ docs/ui.md「共有の中の画面」)。
// 画面の口が API につながっていることだけを見る。権限ごとの可否や、名前の重なりの扱いは API のテストで確かめている。
// 共有の登録はサーバーの PC からしか通らないので、別のマシンのサーバーに当てるときは飛ばす。
import { test, expect, type Page } from '@playwright/test';
import {
	createRootByApi,
	logInByApi,
	removeRootByApi,
	send,
	withUserByApi
} from './api-helpers';
import { createRootFixtureTree, FIXTURE_UNREACHABLE, removeFixtureTree } from './fixture-helpers';
import {
	anonymousContext,
	confirmDelete,
	escapeRegExp,
	requiredLabel,
	uniqueId,
	waitForDialog
} from './helpers';
import { SERVER_IS_REMOTE, SERVER_IS_REMOTE_REASON } from './server-location';

test.skip(SERVER_IS_REMOTE, SERVER_IS_REMOTE_REASON);

const SHARES_HEADING_NAME = /^共有$|^Shares$/;
const ADD_BUTTON_NAME = /^追加$|^Add$/;
const UPLOAD_MENU_ITEM_NAME = /^アップロード\.\.\.$|^Upload\.\.\.$/;
const UPLOAD_BUTTON_NAME = /^アップロードする$|^Upload$/;
const RENAME_MENU_ITEM_NAME = /^名前の変更\.\.\.$|^Rename\.\.\.$/;
const RENAME_BUTTON_NAME = /^変更する$|^Rename$/;
const MOVE_MENU_ITEM_NAME = /^移動\.\.\.$|^Move\.\.\.$/;
const MOVE_BUTTON_NAME = /^ここに移動する$|^Move here$/;
const DOWNLOAD_MENU_ITEM_NAME = /^ダウンロード$|^Download$/;
const DELETE_MENU_ITEM_NAME = /^削除\.\.\.$|^Delete\.\.\.$/;

/** 行の ⋮ のボタン。名前には行の名前が入る (→ docs/ui.md「UI 全般」)。 */
function rowActions(page: Page, name: string) {
	const escaped = escapeRegExp(name);
	return page.getByRole('button', {
		name: new RegExp(`^${escaped}の操作$|^Actions for ${escaped}$`)
	});
}

/**
 * 既定のフィクスチャの木 (2024/・2023/ の下に音声と PDF) を共有に登録し、`body` に渡す。
 * サーバーからパスが見えなければテストを飛ばす。登録と木は、途中で失敗しても片付ける。
 */
async function withShare(
	page: Page,
	body: (share: { id: number; name: string }) => Promise<void>
): Promise<void> {
	const name = uniqueId('e2e-share');
	const root = createRootFixtureTree(name);
	let id: number | null = null;
	try {
		id = await createRootByApi(page.request, root);
		test.skip(id === null, FIXTURE_UNREACHABLE);
		await body({ id: id!, name });
	} finally {
		if (id !== null) await removeRootByApi(page.request, id);
		removeFixtureTree(root);
	}
}

test('共有の中で、アップロード・名前の変更・移動・削除ができる', async ({ page }) => {
	await withShare(page, async (share) => {
		await page.goto(`/shares/${share.id}`);
		await expect(page.getByRole('heading', { level: 1, name: share.name })).toBeVisible();

		await page.getByRole('button', { name: ADD_BUTTON_NAME }).click();
		await page.getByRole('menuitem', { name: UPLOAD_MENU_ITEM_NAME }).click();
		await waitForDialog(page, { awaitAutofocus: false });
		await page.locator('#share-upload-files').setInputFiles({
			name: 'e2e.txt',
			mimeType: 'text/plain',
			buffer: Buffer.from('e2e')
		});
		await page.getByRole('dialog').getByRole('button', { name: UPLOAD_BUTTON_NAME }).click();
		// 置いた後に一覧を取り直すので、行の ⋮ が出れば届いている。
		await expect(rowActions(page, 'e2e.txt')).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(page.getByRole('dialog')).toHaveCount(0);

		await rowActions(page, 'e2e.txt').click();
		await page.getByRole('menuitem', { name: RENAME_MENU_ITEM_NAME }).click();
		await waitForDialog(page);
		await page.getByLabel(requiredLabel('名前', 'Name')).fill('renamed.txt');
		await page.getByRole('dialog').getByRole('button', { name: RENAME_BUTTON_NAME }).click();
		await expect(rowActions(page, 'renamed.txt')).toBeVisible();
		await expect(rowActions(page, 'e2e.txt')).toHaveCount(0);

		// 置き先は今いるフォルダーから選び始める。
		await rowActions(page, 'renamed.txt').click();
		await page.getByRole('menuitem', { name: MOVE_MENU_ITEM_NAME }).click();
		const moveDialog = page.getByRole('dialog');
		await moveDialog.getByRole('button', { name: '2024', exact: true }).click();
		await moveDialog.getByRole('button', { name: MOVE_BUTTON_NAME }).click();
		await expect(rowActions(page, 'renamed.txt')).toHaveCount(0);

		await page.getByRole('link', { name: '2024' }).click();
		await expect(page.getByRole('heading', { level: 1, name: '2024' })).toBeVisible();
		await rowActions(page, 'renamed.txt').click();
		await page.getByRole('menuitem', { name: DELETE_MENU_ITEM_NAME }).click();
		await confirmDelete(page);
		await expect(rowActions(page, 'renamed.txt')).toHaveCount(0);
		await expect(rowActions(page, 'answer.pdf')).toBeVisible();
	});
});

test('追加のみの人には、行の ⋮ に名前の変更が出ない', async ({ page, browser }) => {
	const username = uniqueId('e2e-share-add');
	const password = 'E2eSmoke!2026Test';
	await withShare(page, async (share) => {
		await withUserByApi(page.request, username, password, async (userId) => {
			await send(page.request, 'put', `/admin/roots/${share.id}/access`, {
				editorAccess: 'read',
				users: [{ userId, access: 'add' }]
			});
			const context = await anonymousContext(browser);
			try {
				await logInByApi(context.request, username, password);
				const userPage = await context.newPage();
				await userPage.goto(`/shares/${share.id}?path=2024`);
				await expect(userPage.getByRole('button', { name: ADD_BUTTON_NAME })).toBeVisible();

				await rowActions(userPage, 'answer.pdf').click();
				const menu = userPage.getByRole('menu');
				await expect(menu.getByRole('menuitem', { name: DOWNLOAD_MENU_ITEM_NAME })).toBeVisible();
				await expect(menu.getByRole('menuitem', { name: RENAME_MENU_ITEM_NAME })).toHaveCount(0);
			} finally {
				await context.close();
			}
		});
	});
});

test('ホームの「共有」の区画は、ログインしていない人には出ない', async ({ page, browser }) => {
	// ログインした側に区画が出ることも見て、出ないのが共有の無さのせいでないと分かるようにする。
	await withShare(page, async () => {
		await page.goto('/');
		await expect(page.getByRole('heading', { level: 2, name: SHARES_HEADING_NAME })).toBeVisible();

		const context = await anonymousContext(browser);
		try {
			const anonPage = await context.newPage();
			await anonPage.goto('/');
			await expect(anonPage.getByRole('heading', { level: 1 }).first()).toBeAttached();
			await expect(
				anonPage.getByRole('heading', { level: 2, name: SHARES_HEADING_NAME })
			).toHaveCount(0);
		} finally {
			await context.close();
		}
	});
});
