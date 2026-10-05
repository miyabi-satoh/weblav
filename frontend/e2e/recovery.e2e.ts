// リカバリコードを作り、それでパスワードを再設定する流れ (→ docs/access.md「リカバリコード」)。
// 共有の管理者 (`admin`) のパスワードを変えるとほかのテストのログインが壊れるので、
// 使い捨てのユーザーを API で作って使う。
import { test, expect } from '@playwright/test';
import {
	RECOVERY_CODE_CREATE_BUTTON_NAME,
	CURRENT_PASSWORD_LABEL,
	EMPTY_STORAGE_STATE,
	FORGOT_PASSWORD_LINK_NAME,
	LOGIN_PASSWORD_LABEL,
	LOGIN_USERNAME_LABEL,
	PASSWORD_HASH_TIMEOUT,
	RECOVERY_CODE_MENU_ITEM_NAME,
	USERNAME_LABEL,
	fillLogInForm,
	requiredLabel,
	uniqueId
} from './helpers';
import { keepRecoveryCode } from './recovery-helpers';
import { removeUserIfPresent } from './user-helpers';

const MISSING_NOTICE_TEXT =
	/リカバリコードをまだ作っていません|You haven't created a recovery code yet/;
const CREATE_ACTION_NAME = /^リカバリコードを作る$|^Create recovery code$/;
const RECOVERY_CODE_LABEL = requiredLabel('リカバリコード', 'Recovery code');
const NEW_PASSWORD_LABEL = requiredLabel('新しいパスワード', 'New password');
const NEW_PASSWORD_CONFIRM_LABEL = requiredLabel(
	'新しいパスワード(確認)',
	'New password (confirm)'
);
const RESET_BUTTON_NAME = /^再設定する$|^Reset password$/;
const INCORRECT_CURRENT_TEXT =
	/現在のパスワードが正しくありません|The current password is incorrect/;
const INVALID_RECOVERY_TEXT =
	/ユーザー名かリカバリコードが正しくありません|The username or recovery code is incorrect/;

test('管理者はリカバリコードを作り、パスワードを忘れたらそれで再設定できる', async ({
	page,
	browser
}) => {
	// Argon2 のハッシュ化・照合を8回ほど重ねるので、マシンが混んでいると既定の30秒を超える。
	test.slow();
	const username = uniqueId('e2e-recovery');
	const created = await page.request.post('/api/v1/admin/users', {
		data: { username, password: 'old-password', role: 'admin' }
	});
	expect(created.ok()).toBe(true);

	const context = await browser.newContext({ storageState: EMPTY_STORAGE_STATE });
	const own = await context.newPage();
	try {
		await own.goto('/login');
		await fillLogInForm(own, username, 'old-password');
		await own.waitForURL('/', { timeout: PASSWORD_HASH_TIMEOUT });

		// まだ作っていない管理者には、管理画面に帯を出す。帯から作ると帯は消える。
		await own.goto('/admin/contents');
		await expect(own.getByText(MISSING_NOTICE_TEXT)).toBeVisible();
		await own.getByRole('button', { name: CREATE_ACTION_NAME }).click();
		const dialog = own.getByRole('dialog');
		// 今のパスワードを間違えても、ログインへ飛ばさずその場で知らせる。
		await dialog.getByLabel(CURRENT_PASSWORD_LABEL).fill('wrong-password');
		await dialog.getByRole('button', { name: RECOVERY_CODE_CREATE_BUTTON_NAME }).click();
		await expect(own.getByText(INCORRECT_CURRENT_TEXT)).toBeVisible({
			timeout: PASSWORD_HASH_TIMEOUT
		});
		await own.keyboard.press('Escape');
		await expect(own).toHaveURL('/admin/contents');
		await dialog.getByLabel(CURRENT_PASSWORD_LABEL).fill('old-password');
		await dialog.getByRole('button', { name: RECOVERY_CODE_CREATE_BUTTON_NAME }).click();
		const code = await keepRecoveryCode(own);
		await expect(own.getByText(MISSING_NOTICE_TEXT)).toHaveCount(0);

		// ログアウトして、ログイン画面の「パスワードを忘れた」から戻る。
		await context.clearCookies();
		await own.goto('/login');
		await own.getByRole('link', { name: FORGOT_PASSWORD_LINK_NAME }).click();
		await own.waitForURL('/recover');

		// 違うコードは断る。
		await own.getByLabel(USERNAME_LABEL).fill(username);
		await own.getByLabel(RECOVERY_CODE_LABEL).fill('00000-00000-00000-00000');
		await own.getByLabel(NEW_PASSWORD_LABEL).fill('new-password');
		await own.getByLabel(NEW_PASSWORD_CONFIRM_LABEL).fill('new-password');
		await own.getByRole('button', { name: RESET_BUTTON_NAME }).click();
		await expect(own.getByText(INVALID_RECOVERY_TEXT)).toBeVisible({
			timeout: PASSWORD_HASH_TIMEOUT
		});
		await own.keyboard.press('Escape');

		// 区切りを省いた小文字でも通る。通ると新しいコードを見せ、ログインした状態で管理画面へ移る。
		await own.getByLabel(RECOVERY_CODE_LABEL).fill(code.replaceAll('-', '').toLowerCase());
		await own.getByRole('button', { name: RESET_BUTTON_NAME }).click();
		const newCode = await keepRecoveryCode(own);
		expect(newCode).not.toBe(code);
		await own.waitForURL('/admin/contents');
		await expect(own.getByText(MISSING_NOTICE_TEXT)).toHaveCount(0);

		// 新しいパスワードでログインできる。
		await context.clearCookies();
		await own.goto('/login');
		await own.getByLabel(LOGIN_USERNAME_LABEL).fill(username);
		await own.getByLabel(LOGIN_PASSWORD_LABEL).fill('new-password');
		await own.getByLabel(LOGIN_PASSWORD_LABEL).press('Enter');
		await own.waitForURL('/', { timeout: PASSWORD_HASH_TIMEOUT });
	} finally {
		await context.close();
		await removeUserIfPresent(page, username);
	}
});

test('編集者も、ユーザーメニューから作ったリカバリコードでパスワードを再設定できる', async ({
	page,
	browser
}) => {
	test.slow();
	const username = uniqueId('e2e-recovery-editor');
	const created = await page.request.post('/api/v1/admin/users', {
		data: { username, password: 'old-password', role: 'user' }
	});
	expect(created.ok()).toBe(true);

	const context = await browser.newContext({ storageState: EMPTY_STORAGE_STATE });
	const own = await context.newPage();
	try {
		await own.goto('/login');
		await fillLogInForm(own, username, 'old-password');
		await own.waitForURL('/', { timeout: PASSWORD_HASH_TIMEOUT });

		// 編集者には作るよう促す帯を出さない。管理者に再設定してもらう道もあるため。
		await own.goto('/admin/contents');
		await expect(own.getByRole('heading', { level: 1 })).toBeVisible();
		await expect(own.getByText(MISSING_NOTICE_TEXT)).toHaveCount(0);

		await own.getByRole('button', { name: username, exact: true }).click();
		await own.getByRole('menuitem', { name: RECOVERY_CODE_MENU_ITEM_NAME }).click();
		const dialog = own.getByRole('dialog');
		await dialog.getByLabel(CURRENT_PASSWORD_LABEL).fill('old-password');
		await dialog.getByRole('button', { name: RECOVERY_CODE_CREATE_BUTTON_NAME }).click();
		const code = await keepRecoveryCode(own);

		await context.clearCookies();
		await own.goto('/recover');
		await own.getByLabel(USERNAME_LABEL).fill(username);
		await own.getByLabel(RECOVERY_CODE_LABEL).fill(code);
		await own.getByLabel(NEW_PASSWORD_LABEL).fill('new-password');
		await own.getByLabel(NEW_PASSWORD_CONFIRM_LABEL).fill('new-password');
		await own.getByRole('button', { name: RESET_BUTTON_NAME }).click();
		await keepRecoveryCode(own);
		await own.waitForURL('/admin/contents');
	} finally {
		await context.close();
		await removeUserIfPresent(page, username);
	}
});
