// 未ログイン状態を前提とするスモークテスト。
// e2eプロジェクトの既定storageStateは管理者ログイン済みのため、このファイルでは
// 空のstorageStateに上書きして未ログイン状態から始める。
import { test, expect, type Page } from '@playwright/test';
import { ADMIN_USERNAME, ADMIN_PASSWORD } from './auth';
import {
	CLOSE_BUTTON_NAME,
	EMPTY_STORAGE_STATE,
	FORGOT_PASSWORD_LINK_NAME,
	INVALID_SETUP_LINK_TEXT,
	LOGIN_PASSWORD_LABEL,
	LOGIN_USERNAME_LABEL,
	logIn,
	PASSWORD_HASH_TIMEOUT,
	SETUP_HELP_LINK_TEXT
} from './helpers';

test.use({ storageState: EMPTY_STORAGE_STATE });

/**
 * 表示言語の cookie。`baseURL` は playwright.config.ts が必ず入れるが、型の上では省略できるため
 * 既定のバックエンドの URL を添える。
 */
function localeCookie(name: string, value: string, baseURL: string | undefined) {
	return { name, value, url: baseURL ?? 'http://127.0.0.1:3000' };
}

test('ログインページにユーザー名・パスワード入力欄がある', async ({ page }) => {
	await page.goto('/login');
	await expect(page.getByLabel(LOGIN_USERNAME_LABEL)).toBeVisible();
	await expect(page.getByLabel(LOGIN_PASSWORD_LABEL)).toBeVisible();
});

test('管理者がいればサーバーの PC から開いても、最初の管理者を作る手順への案内を出さない', async ({
	page
}) => {
	await page.goto('/login');
	// 案内の隣のリンクが出てから見る。問い合わせの前に「出ていない」と判定しないため。
	await expect(page.getByRole('link', { name: FORGOT_PASSWORD_LINK_NAME })).toBeVisible();
	await expect(page.getByRole('link', { name: SETUP_HELP_LINK_TEXT })).toHaveCount(0);
});

test('どのページにも当たらない URL は、見出しとホームへの戻り先を出す', async ({ page }) => {
	await page.goto('/no/such/page');
	await expect(
		page.getByRole('heading', { level: 1, name: /^ページが見つかりません$|^Page not found$/ })
	).toBeVisible();
	await page.getByRole('link', { name: /^ホームへ戻る$|^Back to home$/ }).click();
	await page.waitForURL((url) => url.pathname === '/');
});

test('開けないファイルを直接開いても、JSON ではなく知らせが出る', async ({ page }) => {
	// ブラウザは <a target="_blank"> で API の URL をそのまま開く。生の envelope を
	// タブに出すと戻る手がかりが無くなるため、画面へ送る (→ src/api/browser.rs)。
	await page.goto('/api/v1/contents/999999/download');
	await expect(page.getByRole('alertdialog')).toBeVisible();

	// 知らせたあとはクエリを落とす。読み込み直しで蘇らせない。
	// 落とすのは非同期なので待つ。
	await page.waitForURL((url) => url.pathname === '/' && !url.searchParams.has('error'));
	await page.getByRole('button', { name: CLOSE_BUTTON_NAME }).click();
	await page.reload();
	await expect(page.getByRole('alertdialog')).toHaveCount(0);
});

test('誤ったパスワードでログインするとエラーダイアログが出る', async ({ page }) => {
	await logIn(page, ADMIN_USERNAME, `${ADMIN_PASSWORD}-wrong`);
	await expect(page.getByRole('alertdialog')).toBeVisible({ timeout: PASSWORD_HASH_TIMEOUT });
});

test('セットアップ: トークンの無いリンクではフォームを出さない', async ({ page }) => {
	// 管理者が既にいるので、トークンがあっても断られる状態 (→ docs/access.md「初回セットアップ」)。
	await page.goto('/setup');
	await expect(page.getByText(INVALID_SETUP_LINK_TEXT)).toBeVisible();
	await expect(page.getByLabel(LOGIN_USERNAME_LABEL)).toHaveCount(0);
});

test('マニュアル: 未ログインでも目次が表示される', async ({ page }) => {
	await page.goto('/help');
	await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible();
});

test('マニュアル: 個別ページが表示される', async ({ page }) => {
	await page.goto('/help/setup');
	await expect(page.locator('.help-body')).toBeVisible();
	await expect(page.locator('.help-body')).not.toBeEmpty();
});

test('マニュアル: スマートフォン幅の個別ページでは目次をたたむ', async ({ page }) => {
	// 本文から見せ、目次はボタンで開く。ページを移ったらたたみ直す (→ docs/help.md)。
	await page.setViewportSize({ width: 390, height: 844 });
	await page.goto('/help/setup');
	const toc = page.locator('#help-toc');
	await expect(toc).toBeHidden();
	await page.getByRole('button', { name: 'Contents' }).click();
	await expect(toc).toBeVisible();
	await toc.getByRole('link', { name: 'Add shared folders' }).click();
	await expect(page).toHaveURL(/\/help\/roots$/);
	await expect(toc).toBeHidden();
});

test('マニュアル: 画面の表示言語で本文が返る', async ({ page, context, baseURL }) => {
	// マニュアルの言語は画面の表示言語に従う (→ docs/help.md)。
	// cookie が無いときはブラウザの言語設定で決まり、playwright.config.ts が en に固定している。
	await page.goto('/help/setup');
	await expect(page.locator('.help-body')).toHaveAttribute('lang', 'en');
	await expect(
		page.locator('.help-body').getByRole('link', { name: 'Add shared folders' })
	).toBeVisible();

	// 表示言語を日本語にすると、同じ URL のまま本文も目次も日本語になる。
	// ここを見ないと baseLocale に落ちているだけの状態と区別が付かない。
	await context.addCookies([localeCookie('WEBLAV_LOCALE', 'ja', baseURL)]);
	await page.goto('/help/setup');
	await expect(page.locator('.help-body')).toHaveAttribute('lang', 'ja');
	await expect(
		page.locator('.help-body').getByRole('link', { name: '公開できるフォルダを追加する' })
	).toBeVisible();
});

test('ライセンス: 未ログインでもアプリ本体と画面の両方が出る', async ({ page }) => {
	// 出どころが2つあり、片方が欠けてもページは出る作りなので、両方から1つずつ見る
	// (→ docs/third-party-licenses.md)。
	await page.goto('/licenses');
	await expect(licenseRow(page, 'option-ext')).toBeVisible();
	await expect(licenseRow(page, 'svelte')).toBeVisible();
	// 式は語ごとに組むので、語の間の空白が落ちていないかも見る。
	await expect(licenseRow(page, 'argon2').locator('summary')).toContainText('MIT OR Apache-2.0');
});

test('ライセンス: 行を開くと、ソースの置き場所と本文が出る', async ({ page }) => {
	await page.goto('/licenses');
	// MPL-2.0 の依存。受け取る人にソースの入手先を知らせる必要がある (→ docs/third-party-licenses.md)。
	const row = licenseRow(page, 'option-ext');
	await expect(row.getByRole('link')).toHaveCount(0);
	await row.locator('summary').click();
	await expect(row.getByRole('link', { name: /^https:\/\// })).toBeVisible();
	await expect(row.getByText(/Mozilla Public License/).first()).toBeVisible();
});

test('ライセンス: マニュアルの目次から移れる', async ({ page }) => {
	await page.goto('/help');
	await page.getByRole('link', { name: 'Third-party software licenses' }).click();
	await expect(page.getByRole('heading', { name: 'Third-party software' })).toBeVisible();
});

// 初回の表示言語はブラウザの言語設定で決まる (→ docs/ui.md「UI 全般」)。
// strategy の並びや cookie 名を取り違えても画面は出てしまうので、言語を変えた context で確かめる。
// `<html lang>` は起動時に解決した言語を映す (→ `src/routes/+layout.svelte`)。
test.describe('初回の表示言語', () => {
	test.use({ locale: 'ja-JP' });

	test('cookie が無ければブラウザの言語設定に従う', async ({ page }) => {
		await page.goto('/');
		await expect(page.locator('html')).toHaveAttribute('lang', 'ja');
	});

	test('WEBLAV_LOCALE はブラウザの言語設定より優先する', async ({ page, context, baseURL }) => {
		await context.addCookies([localeCookie('WEBLAV_LOCALE', 'en', baseURL)]);
		await page.goto('/');
		await expect(page.locator('html')).toHaveAttribute('lang', 'en');
	});
});

test.describe('翻訳を持たない言語のブラウザ', () => {
	test.use({ locale: 'de-DE' });

	test('翻訳が無ければ baseLocale の英語にする', async ({ page }) => {
		await page.goto('/');
		await expect(page.locator('html')).toHaveAttribute('lang', 'en');
	});
});

/** /licenses の、名前が `name` のパッケージの行。 */
function licenseRow(page: Page, name: string) {
	return page.locator('details').filter({
		has: page.locator('summary').getByText(name, { exact: true })
	});
}
