// 初回セットアップで最初の管理者を作る流れ (→ docs/access.md「初回セットアップ」)。
//
// 管理者が0人のサーバーでしか通せないので、ほかのテストとは別の、使い捨ての backend に当てる。
// `just e2e-local` (scripts/run-e2e.ts) が立てて `E2E_SETUP_BASE_URL` で渡す。
// 起動済みの開発サーバーに当てる `just e2e` では管理者がいるので、このテストは飛ばす。
// 管理者を作ると二度と通せないので、1本の中で失敗と成功を続けて見る。
import { test, expect } from '@playwright/test';
import {
	EMPTY_STORAGE_STATE,
	FORGOT_PASSWORD_LINK_NAME,
	INVALID_SETUP_LINK_TEXT,
	PASSWORD_CONFIRM_LABEL,
	PASSWORD_LABEL,
	PASSWORD_MISMATCH_TEXT,
	SETUP_HELP_LINK_TEXT,
	USERNAME_LABEL
} from './helpers';
import { keepRecoveryCode } from './recovery-helpers';

const SETUP_BASE_URL = process.env.E2E_SETUP_BASE_URL;

test.use({ storageState: EMPTY_STORAGE_STATE });

const SUBMIT_BUTTON_NAME = /^管理者を作成する$|^Create administrator$/;

test('トレイから開いたリンクで最初の管理者を作ると、リカバリコードを見せてから、ログインした状態でコンテンツ管理へ移る', async ({
	page,
	request
}) => {
	test.skip(!SETUP_BASE_URL, 'E2E_SETUP_BASE_URL が無い (管理者が0人のサーバーを用意していない)');

	// 管理者がいないうちは、ログイン画面に最初の管理者を作る手順への案内を出す。
	await page.goto(`${SETUP_BASE_URL}/login`);
	await expect(page.getByRole('link', { name: SETUP_HELP_LINK_TEXT })).toHaveAttribute(
		'href',
		/\/help\/setup$/
	);

	// トレイの「セットアップ」と同じ口でトークンを受け取る。この PC の中からの要求だけが通る。
	const issued = await request.post(`${SETUP_BASE_URL}/api/v1/setup/token`);
	expect(issued.ok()).toBe(true);
	const token = await issued.text();
	const setupUrl = `${SETUP_BASE_URL}/setup?token=${encodeURIComponent(token)}`;

	// 違うトークンではフォームを出さない。
	await page.goto(`${SETUP_BASE_URL}/setup?token=${'0'.repeat(token.length)}`);
	await expect(page.getByText(INVALID_SETUP_LINK_TEXT)).toBeVisible();
	await expect(page.getByLabel(USERNAME_LABEL)).toHaveCount(0);

	await page.goto(setupUrl);
	await page.getByLabel(USERNAME_LABEL).fill('e2e-first-admin');
	await page.getByLabel(PASSWORD_LABEL).fill('E2eSetup!2026');

	// 確認欄が一致しないうちは送れない。
	await page.getByLabel(PASSWORD_CONFIRM_LABEL).fill('E2eSetup!2026-other');
	await expect(page.getByText(PASSWORD_MISMATCH_TEXT)).toBeVisible();
	await expect(page.getByRole('button', { name: SUBMIT_BUTTON_NAME })).toBeDisabled();

	await page.getByLabel(PASSWORD_CONFIRM_LABEL).fill('E2eSetup!2026');
	await page.getByRole('button', { name: SUBMIT_BUTTON_NAME }).click();

	// リカバリコードを保管してから管理画面へ移る (→ docs/access.md「リカバリコード」)。
	await keepRecoveryCode(page);
	await page.waitForURL(`${SETUP_BASE_URL}/admin/contents`);
	await expect(
		page.getByRole('heading', { level: 1, name: /^コンテンツ管理$|^Manage contents$/ })
	).toBeVisible();

	// トークンは使い切り。同じリンクを開き直してもフォームは出ない。
	await page.goto(setupUrl);
	await expect(page.getByText(INVALID_SETUP_LINK_TEXT)).toBeVisible();

	// 管理者ができたら、ログイン画面の案内は消える。
	await page.context().clearCookies();
	await page.goto(`${SETUP_BASE_URL}/login`);
	await expect(page.getByRole('link', { name: FORGOT_PASSWORD_LINK_NAME })).toBeVisible();
	await expect(page.getByRole('link', { name: SETUP_HELP_LINK_TEXT })).toHaveCount(0);
});
