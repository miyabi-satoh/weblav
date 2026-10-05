// 入力を受け付けなかったときの画面の出方。
// サーバーが 422 を返すこと自体は tests/api/ が見ている。ここでは、それが画面に届き、
// 入力し直せる状態で残ることを見る。
import { test, expect, type Page } from '@playwright/test';
import { send, withUserByApi } from './api-helpers';
import { createLinkContent, openCreateForm, removeContentIfPresent } from './content-helpers';
import {
	clickRowMenuItem,
	CLOSE_BUTTON_NAME,
	CONTENT_SUBMIT_BUTTON_NAME,
	DELETE_MENU_ITEM_NAME,
	ERROR_DIALOG_NAME,
	INVALID_INPUT_TEXT,
	openAddDialog,
	PASSWORD_CONFIRM_LABEL,
	PASSWORD_HASH_TIMEOUT,
	PASSWORD_LABEL,
	PASSWORD_MISMATCH_TEXT,
	RESET_PASSWORD_MENU_ITEM_NAME,
	ROW_ACTIONS_BUTTON_NAME,
	SAVE_BUTTON_NAME,
	TITLE_LABEL,
	uniqueId,
	USER_ADD_BUTTON_NAME,
	USERNAME_LABEL,
	userRow,
	waitForDialog
} from './helpers';
import { createUser, removeUserIfPresent } from './user-helpers';

/**
 * エラーダイアログに `text` が出ていることを確かめて閉じる。
 * パスワードをハッシュ化してから失敗する操作は、`timeout` に `PASSWORD_HASH_TIMEOUT` を渡す。
 */
async function expectErrorDialog(page: Page, text: RegExp, timeout?: number) {
	const dialog = page.getByRole('alertdialog', { name: ERROR_DIALOG_NAME });
	await expect(dialog.getByText(text)).toBeVisible({ timeout });
	await dialog.getByRole('button', { name: CLOSE_BUTTON_NAME }).click();
	await expect(dialog).toHaveCount(0);
}

test.describe('コンテンツ', () => {
	test('作成: タイトルが空白だけだと知らせ、作成ダイアログは入力を残したまま開いている', async ({
		page
	}) => {
		// タイトルを入力させるのは group だけ (ほかは自動で付く)。
		await openCreateForm(page, 'group');
		// 空白だけは `required` を通るので、サーバーまで届いて断られる。
		await page.getByLabel(TITLE_LABEL).fill('   ');
		await page.getByRole('button', { name: CONTENT_SUBMIT_BUTTON_NAME }).click();

		await expectErrorDialog(page, INVALID_INPUT_TEXT);
		await expect(page).toHaveURL(/\/admin\/contents$/);
		await expect(page.getByLabel(TITLE_LABEL)).toHaveValue('   ');
	});

	test('編集: タイトルを空白だけにして保存すると知らせ、保存されない', async ({ page }) => {
		const title = uniqueId('E2E検証');
		try {
			await createLinkContent(page, title);
			await page.getByLabel(TITLE_LABEL).fill('   ');
			await page.getByRole('button', { name: SAVE_BUTTON_NAME }).click();

			await expectErrorDialog(page, INVALID_INPUT_TEXT);
			// サーバー側も元のタイトルのまま。
			// 画面を読み込み直して確かめると、未保存の変更の確認に止められるので API で見る。
			const id = Number(new URL(page.url()).pathname.split('/').pop());
			const contents = (await send(page.request, 'get', '/admin/contents')) as {
				id: number;
				title: string;
			}[];
			expect(contents.find((content) => content.id === id)?.title).toBe(title);
			// 元に戻して未保存の変更を無くし、後片付けで画面を移れるようにする。
			await page.getByLabel(TITLE_LABEL).fill(title);
		} finally {
			await removeContentIfPresent(page, title);
		}
	});
});

test.describe('ユーザー', () => {
	test('追加: 確認欄が一致しないと送らず、欄の下に理由が出る', async ({ page }) => {
		const username = uniqueId('e2e-mismatch');
		try {
			await page.goto('/admin/users');
			await openAddDialog(page, USER_ADD_BUTTON_NAME);
			await page.getByLabel(USERNAME_LABEL).fill(username);
			await page.getByLabel(PASSWORD_LABEL).fill('E2eSmoke!2026Test');
			await page.getByLabel(PASSWORD_CONFIRM_LABEL).fill('E2eSmoke!2026Other');
			await page.getByRole('button', { name: SAVE_BUTTON_NAME }).click();

			const dialog = page.getByRole('dialog');
			await expect(dialog.getByText(PASSWORD_MISMATCH_TEXT)).toBeVisible();
			await expect(dialog.getByLabel(PASSWORD_CONFIRM_LABEL)).toHaveAttribute(
				'aria-invalid',
				'true'
			);

			// 作られていない。
			await page.reload();
			await page.getByRole('heading', { level: 1 }).first().waitFor();
			await expect(userRow(page, username)).toHaveCount(0);
		} finally {
			await removeUserIfPresent(page, username);
		}
	});

	test('追加: 使われているユーザー名だと知らせ、ダイアログは開いたまま', async ({ page }) => {
		const username = uniqueId('e2e-taken');
		await withUserByApi(page.request, username, 'E2eSmoke!2026Test', async () => {
			await page.goto('/admin/users');
			await openAddDialog(page, USER_ADD_BUTTON_NAME);
			await page.getByLabel(USERNAME_LABEL).fill(username);
			await page.getByLabel(PASSWORD_LABEL).fill('E2eSmoke!2026Test');
			await page.getByLabel(PASSWORD_CONFIRM_LABEL).fill('E2eSmoke!2026Test');
			await page.getByRole('button', { name: SAVE_BUTTON_NAME }).click();

			// 名前が重複していても、ハッシュ化を済ませてから失敗する (src/auth.rs の create_user)。
			await expectErrorDialog(
				page,
				new RegExp(
					`ユーザー名「${username}」は既に使われています|The username "${username}" is already taken`
				),
				PASSWORD_HASH_TIMEOUT
			);
			await expect(page.getByRole('dialog').getByLabel(USERNAME_LABEL)).toHaveValue(username);
			await expect(userRow(page, username)).toHaveCount(1);
		});
	});

	test('パスワード再設定: 確認欄が一致しないと送らず、欄の下に理由が出る', async ({ page }) => {
		const username = uniqueId('e2e-reset-mismatch');
		try {
			await createUser(page, username, 'E2eSmoke!2026Test');
			await clickRowMenuItem(page, userRow(page, username), RESET_PASSWORD_MENU_ITEM_NAME);
			await waitForDialog(page);
			await page.getByLabel(PASSWORD_LABEL).fill('E2eSmoke!2026Reset');
			await page.getByLabel(PASSWORD_CONFIRM_LABEL).fill('E2eSmoke!2026Other');
			await page.getByRole('button', { name: SAVE_BUTTON_NAME }).click();

			const dialog = page.getByRole('dialog');
			await expect(dialog.getByText(PASSWORD_MISMATCH_TEXT)).toBeVisible();
			await expect(dialog).toBeVisible();
		} finally {
			await removeUserIfPresent(page, username);
		}
	});

	test('最後の管理者は、権限を変えられず削除もできない', async ({ page }) => {
		const users = (await send(page.request, 'get', '/admin/users')) as { role: string }[];
		// 開発用サーバーに当てる `just e2e` では、管理者が複数いることがある。
		test.skip(users.filter((user) => user.role === 'admin').length !== 1, '管理者が1人ではない');

		await page.goto('/admin/users');
		const row = page.getByRole('row').filter({
			hasText: /最後の管理者は降格・削除できません|The last admin cannot be demoted or deleted/
		});
		await expect(row).toHaveCount(1);
		await expect(
			row.getByRole('button', { name: /の権限: 管理者$|^Role for .+: Admin$/ })
		).toBeDisabled();

		await row.getByRole('button', { name: ROW_ACTIONS_BUTTON_NAME }).click();
		await expect(
			page.getByRole('menu').getByRole('menuitem', { name: DELETE_MENU_ITEM_NAME })
		).toBeDisabled();
	});
});
