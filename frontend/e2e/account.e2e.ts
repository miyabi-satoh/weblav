// 自分のアカウントのページと、操作の前のパスワードの確かめ直し
// (→ docs/ui.md「アカウントのページ」・docs/access.md「操作の前のパスワードの確認」)。
// 共有の管理者の名前やパスワードを変えるとほかのテストのログインが壊れるので、使い捨てのユーザーで見る。
import { test, expect, type Page, type Route } from '@playwright/test';
import { logInByApi, withUserByApi } from './api-helpers';
import {
	CANCEL_BUTTON_NAME,
	CURRENT_PASSWORD_LABEL,
	ERROR_DIALOG_NAME,
	LOGIN_PASSWORD_LABEL,
	LOGIN_USERNAME_LABEL,
	NEW_PASSWORD_CONFIRM_LABEL,
	NEW_PASSWORD_LABEL,
	PASSWORD_HASH_TIMEOUT,
	SAVE_BUTTON_NAME,
	USERNAME_LABEL,
	anonymousContext,
	uniqueId
} from './helpers';

const PASSWORD = 'account-password';
const PASSWORD_SECTION_NAME = /^パスワード$|^Password$/;
const CONFIRMATION_DIALOG_NAME = /^パスワードの確認$|^Confirm your password$/;
const CONFIRM_BUTTON_NAME = /^確認する$|^Confirm$/;
const INCORRECT_CURRENT_TEXT =
	/現在のパスワードが正しくありません|The current password is incorrect/;

/**
 * `PUT /auth/username` の最初の1回だけ、確かめ直しが要ると断らせる。ログインしたばかりのセッションは
 * 確かめた内に入るので、時間がたった状態を作る代わりにする。2回目からはサーバーへ通す。
 */
async function requireConfirmationOnce(page: Page): Promise<() => number> {
	let calls = 0;
	await page.route('**/api/v1/auth/username', async (route: Route) => {
		calls += 1;
		if (calls === 1) {
			await route.fulfill({
				status: 403,
				json: { error: { code: 'password_confirmation_required', message: '' } }
			});
			return;
		}
		await route.continue();
	});
	return () => calls;
}

test('アカウントのページで、ユーザー名とパスワードを変えられる', async ({ page, browser }) => {
	test.slow();
	const username = uniqueId('e2e-account');
	const renamed = `${username}-new`;
	await withUserByApi(page.request, username, PASSWORD, async () => {
		const context = await anonymousContext(browser);
		try {
			await logInByApi(context.request, username, PASSWORD);
			const own = await context.newPage();
			await own.goto('/account');

			const usernameForm = own.getByRole('form', { name: USERNAME_LABEL });
			await usernameForm.getByLabel(USERNAME_LABEL).fill(renamed);
			await usernameForm.getByRole('button', { name: SAVE_BUTTON_NAME }).click();
			// ヘッダーの名前も新しくなる。
			await expect(own.getByRole('button', { name: renamed, exact: true })).toBeVisible();

			const passwordForm = own.getByRole('form', { name: PASSWORD_SECTION_NAME });
			await passwordForm.getByLabel(NEW_PASSWORD_LABEL).fill('changed-password');
			await passwordForm.getByLabel(NEW_PASSWORD_CONFIRM_LABEL).fill('changed-password');
			await passwordForm.getByRole('button', { name: SAVE_BUTTON_NAME }).click();
			await expect(passwordForm.getByLabel(NEW_PASSWORD_LABEL)).toHaveValue('', {
				timeout: PASSWORD_HASH_TIMEOUT
			});

			// ログインし直すのは別の窓で。同じ窓で cookie を消すと、保存の後の取り直しの応答が遅れて届き、
			// ログインの cookie を書き戻すことがある。
			const fresh = await anonymousContext(browser);
			try {
				const login = await fresh.newPage();
				await login.goto('/login');
				await login.getByLabel(LOGIN_USERNAME_LABEL).fill(renamed);
				await login.getByLabel(LOGIN_PASSWORD_LABEL).fill('changed-password');
				await login.getByLabel(LOGIN_PASSWORD_LABEL).press('Enter');
				await login.waitForURL('/', { timeout: PASSWORD_HASH_TIMEOUT });
			} finally {
				await fresh.close();
			}
		} finally {
			await context.close();
		}
	});
});

test('確かめ直しが要ると、パスワードを聞いてから同じ操作をやり直す', async ({ page, browser }) => {
	test.slow();
	const username = uniqueId('e2e-confirm');
	const renamed = `${username}-new`;
	await withUserByApi(page.request, username, PASSWORD, async () => {
		const context = await anonymousContext(browser);
		try {
			await logInByApi(context.request, username, PASSWORD);
			const own = await context.newPage();
			const calls = await requireConfirmationOnce(own);
			await own.goto('/account');

			const usernameForm = own.getByRole('form', { name: USERNAME_LABEL });
			await usernameForm.getByLabel(USERNAME_LABEL).fill(renamed);
			await usernameForm.getByRole('button', { name: SAVE_BUTTON_NAME }).click();

			const dialog = own.getByRole('dialog', { name: CONFIRMATION_DIALOG_NAME });
			// 違うパスワードは、閉じずに欄の下へ出す。
			await dialog.getByLabel(CURRENT_PASSWORD_LABEL).fill('wrong-password');
			await dialog.getByRole('button', { name: CONFIRM_BUTTON_NAME }).click();
			await expect(dialog.getByText(INCORRECT_CURRENT_TEXT)).toBeVisible({
				timeout: PASSWORD_HASH_TIMEOUT
			});
			await dialog.getByLabel(CURRENT_PASSWORD_LABEL).fill(PASSWORD);
			await dialog.getByRole('button', { name: CONFIRM_BUTTON_NAME }).click();

			// 確かめられたら、同じ要求を送り直して保存する。
			await expect(dialog).toHaveCount(0, { timeout: PASSWORD_HASH_TIMEOUT });
			await expect(own.getByRole('button', { name: renamed, exact: true })).toBeVisible();
			expect(calls()).toBe(2);
		} finally {
			await context.close();
		}
	});
});

test('確かめ直しをやめると、失敗を知らせずに書きかけを残す', async ({ page, browser }) => {
	const username = uniqueId('e2e-confirm-cancel');
	await withUserByApi(page.request, username, PASSWORD, async () => {
		const context = await anonymousContext(browser);
		try {
			await logInByApi(context.request, username, PASSWORD);
			const own = await context.newPage();
			await requireConfirmationOnce(own);
			await own.goto('/account');

			const usernameForm = own.getByRole('form', { name: USERNAME_LABEL });
			await usernameForm.getByLabel(USERNAME_LABEL).fill(`${username}-draft`);
			await usernameForm.getByRole('button', { name: SAVE_BUTTON_NAME }).click();
			const dialog = own.getByRole('dialog', { name: CONFIRMATION_DIALOG_NAME });
			await dialog.getByRole('button', { name: CANCEL_BUTTON_NAME }).click();

			await expect(dialog).toHaveCount(0);
			await expect(own.getByRole('alertdialog', { name: ERROR_DIALOG_NAME })).toHaveCount(0);
			await expect(usernameForm.getByLabel(USERNAME_LABEL)).toHaveValue(`${username}-draft`);
		} finally {
			await context.close();
		}
	});
});
