// リカバリコードを作り、それでパスワードを再設定する流れ (→ docs/access.md「リカバリコード」)。
// 共有の管理者 (`admin`) のパスワードを変えるとほかのテストのログインが壊れるので、
// 使い捨てのユーザーを API で作って使う。
import { test, expect } from '@playwright/test';
import {
	ACCOUNT_MENU_ITEM_NAME,
	RECOVERY_CODE_CREATE_BUTTON_NAME,
	EMPTY_STORAGE_STATE,
	FORGOT_PASSWORD_LINK_NAME,
	LOGIN_PASSWORD_LABEL,
	LOGIN_USERNAME_LABEL,
	PASSWORD_HASH_TIMEOUT,
	USERNAME_LABEL,
	fillLogInForm,
	requiredLabel,
	uniqueId
} from './helpers';
import { keepRecoveryCode } from './recovery-helpers';
import { removeUserIfPresent } from './user-helpers';

const MISSING_NOTICE_TEXT =
	/リカバリコードがまだ作成されていません|You haven't created a recovery code yet/;
const MISSING_NOTICE_ON_ACCOUNT_TEXT = /まだ作成していません|Not created yet/;
const CREATE_ACTION_NAME = /^リカバリコードを作成\.\.\.$|^Create recovery code$/;
const RECOVERY_CODE_LABEL = requiredLabel('リカバリコード', 'Recovery code');
const NEW_PASSWORD_LABEL = requiredLabel('新しいパスワード', 'New password');
const NEW_PASSWORD_CONFIRM_LABEL = requiredLabel(
	'新しいパスワード (確認)',
	'New password (confirm)'
);
const RESET_BUTTON_NAME = /^パスワードを再設定する$|^Reset password$/;
const RECOVERY_SECTION_NAME = /^リカバリコード$|^Recovery code$/;
const CREATED_ON_TEXT = /に作成しました|Created on /;
const REPLACE_BUTTON_NAME = /^作り直す\.\.\.$|^Regenerate\.\.\.$/;
const INVALID_RECOVERY_TEXT =
	/ユーザー名またはリカバリコードが正しくありません|The username or recovery code is incorrect/;

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
		// ログインしたばかりなので、パスワードは聞き直さずに作る (→ docs/access.md「操作の前のパスワードの確認」)。
		await own.goto('/admin/contents');
		await expect(own.getByText(MISSING_NOTICE_TEXT)).toBeVisible();
		await own.getByRole('button', { name: CREATE_ACTION_NAME }).click();
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

test('編集者も、アカウントのページで作ったリカバリコードでパスワードを再設定できる', async ({
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

		// 作るよう促す帯は管理画面にだけ出し、管理画面は編集者に開かない。編集者はアカウントのページで作る。
		await own.getByRole('button', { name: username, exact: true }).click();
		await own.getByRole('menuitem', { name: ACCOUNT_MENU_ITEM_NAME }).click();
		await own.waitForURL('/account');
		const section = own.getByRole('region', { name: RECOVERY_SECTION_NAME });
		await expect(section).toContainText(MISSING_NOTICE_ON_ACCOUNT_TEXT);
		await section.getByRole('button', { name: RECOVERY_CODE_CREATE_BUTTON_NAME }).click();
		const code = await keepRecoveryCode(own);
		// 作った日を出す。手元の控えの作成日時と見比べられるように。
		await expect(section).toContainText(CREATED_ON_TEXT);
		// 次に押すと、控えたばかりのコードを無効にする前に確かめる形になる。
		await expect(section.getByRole('button', { name: REPLACE_BUTTON_NAME })).toBeVisible();

		await context.clearCookies();
		await own.goto('/recover');
		await own.getByLabel(USERNAME_LABEL).fill(username);
		await own.getByLabel(RECOVERY_CODE_LABEL).fill(code);
		await own.getByLabel(NEW_PASSWORD_LABEL).fill('new-password');
		await own.getByLabel(NEW_PASSWORD_CONFIRM_LABEL).fill('new-password');
		await own.getByRole('button', { name: RESET_BUTTON_NAME }).click();
		await keepRecoveryCode(own);
		// 管理画面は admin 専用なので、編集者はホームへ移る。
		await own.waitForURL('/');
	} finally {
		await context.close();
		await removeUserIfPresent(page, username);
	}
});
