import { expect, type Page } from '@playwright/test';
import {
	clickRowMenuItem,
	confirmDelete,
	DELETE_MENU_ITEM_NAME,
	openAddDialog,
	PASSWORD_CONFIRM_LABEL,
	PASSWORD_HASH_TIMEOUT,
	PASSWORD_LABEL,
	SAVE_BUTTON_NAME,
	USER_ADD_BUTTON_NAME,
	USERNAME_LABEL,
	userRow
} from './helpers';

export async function createUser(page: Page, username: string, password: string) {
	await page.goto('/admin/users');
	await openAddDialog(page, USER_ADD_BUTTON_NAME);
	// roleは既定で"user"(一般ユーザーの作成を確認できれば十分なため)。
	await page.getByLabel(USERNAME_LABEL).fill(username);
	await page.getByLabel(PASSWORD_LABEL).fill(password);
	await page.getByLabel(PASSWORD_CONFIRM_LABEL).fill(password);
	await page.getByRole('button', { name: SAVE_BUTTON_NAME }).click();
	await expect(userRow(page, username)).toBeVisible({ timeout: PASSWORD_HASH_TIMEOUT });
}

export async function deleteUserByUsername(page: Page, username: string) {
	await page.goto('/admin/users');
	const row = userRow(page, username);
	await clickRowMenuItem(page, row, DELETE_MENU_ITEM_NAME);
	await confirmDelete(page);
	await expect(row).toHaveCount(0);
}

export async function removeUserIfPresent(page: Page, username: string) {
	try {
		await page.goto('/admin/users');
		await page.getByRole('heading', { level: 1 }).first().waitFor();
		if ((await userRow(page, username).count()) > 0) {
			await deleteUserByUsername(page, username);
		}
	} catch (err) {
		console.warn(`ユーザーの後片付けに失敗しました (${username}):`, err);
	}
}
