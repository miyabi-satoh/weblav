// バックアップから戻す画面の流れ (→ docs/access.md「バックアップとリストア」)。
//
// 戻すとデータが丸ごと置き換わり、全員のログインも切れるので、ほかのテストとは別の、使い捨ての backend に当てる。
// `just e2e-local` (scripts/run-e2e.ts) が、管理者を入れたものを `E2E_RESTORE_BASE_URL` で、
// 管理者のいないものを `E2E_RESTORE_SETUP_BASE_URL` で渡す。
// 戻した中身とログインが切れることの場合分けは tests/api/backup.rs で見る。ここでは画面から、取る・選ぶ・確かめる・
// ログインへ移る、がつながることを見る。後のテストは前のテストが取ったファイルを使うので、続けて流す。
import { test, expect, type Page } from '@playwright/test';
import { send } from './api-helpers';
import { ADMIN_PASSWORD, ADMIN_USERNAME } from './auth';
import { EMPTY_STORAGE_STATE, logIn, PASSWORD_HASH_TIMEOUT, uniqueId } from './helpers';

const RESTORE_BASE_URL = process.env.E2E_RESTORE_BASE_URL;
const RESTORE_SETUP_BASE_URL = process.env.E2E_RESTORE_SETUP_BASE_URL;

const DOWNLOAD_BUTTON_NAME = /^バックアップをダウンロード$|^Download a backup$/;
const RESTORE_BUTTON_NAME = /^バックアップから復元\.\.\.$|^Restore from a backup\.\.\.$/;
const RESTORE_CONFIRM_BUTTON_NAME = /^復元する$|^Restore$/;

test.use({ storageState: EMPTY_STORAGE_STATE });
test.describe.configure({ mode: 'serial' });

// 前のテストが取ったバックアップと、それに入れたリンクの名前。後のテストが使う。
// 名前は回ごとに変える。流し直すと、戻したときに前の回のリンクも戻り、混ざるため
let backupPath = '';
let keptTitle = '';

async function logInAsAdmin(page: Page) {
	await logIn(page, ADMIN_USERNAME, ADMIN_PASSWORD);
	await page.waitForURL('/', { timeout: PASSWORD_HASH_TIMEOUT });
}

async function contentTitles(page: Page): Promise<string[]> {
	const contents = (await send(page.request, 'get', '/contents')) as { title: string }[];
	return contents.map((content) => content.title);
}

/** 「バックアップから復元...」でファイルを選び、確かめの窓で復元する */
async function restoreFrom(page: Page, file: string) {
	const chooser = page.waitForEvent('filechooser');
	await page.getByRole('button', { name: RESTORE_BUTTON_NAME }).click();
	await (await chooser).setFiles(file);
	await page
		.getByRole('alertdialog')
		.getByRole('button', { name: RESTORE_CONFIRM_BUTTON_NAME })
		.click();
}

test.beforeEach(() => {
	test.skip(
		!RESTORE_BASE_URL || !RESTORE_SETUP_BASE_URL,
		'E2E_RESTORE_BASE_URL・E2E_RESTORE_SETUP_BASE_URL が無い (戻してよい backend を用意していない)'
	);
});

test.describe('管理者のいるサーバー', () => {
	test.use({ baseURL: RESTORE_BASE_URL });

	test('サイト設定でダウンロードしたバックアップから戻すと、入れたものが戻り、ログインし直しになる', async ({
		page
	}, testInfo) => {
		keptTitle = uniqueId('e2e-バックアップに入るリンク');
		await logInAsAdmin(page);
		const kept = (await send(page.request, 'post', '/contents', {
			type: 'link',
			title: keptTitle,
			url: 'https://example.com/e2e'
		})) as { id: number };

		await page.goto('/admin/settings');
		const download = page.waitForEvent('download');
		await page.getByRole('button', { name: DOWNLOAD_BUTTON_NAME }).click();
		backupPath = testInfo.outputPath('backup.zip');
		await (await download).saveAs(backupPath);

		await send(page.request, 'delete', `/contents/${kept.id}`);
		expect(await contentTitles(page)).not.toContain(keptTitle);

		await restoreFrom(page, backupPath);
		await expect(page).toHaveURL(/\/login$/);
		expect((await page.request.get('/api/v1/auth/me')).status(), 'ログインが切れている').toBe(401);

		await logInAsAdmin(page);
		expect(await contentTitles(page)).toContain(keptTitle);
	});
});

test.describe('管理者のいないサーバー', () => {
	test.use({ baseURL: RESTORE_SETUP_BASE_URL });

	test('セットアップの画面から、バックアップで戻せる', async ({ page, request }) => {
		expect(backupPath, '前のテストでバックアップを取っている').not.toBe('');
		// トレイの「セットアップ」と同じ口でトークンを受け取る。この PC の中からの要求だけが通る。
		const issued = await request.post('/api/v1/setup/token');
		expect(issued.ok()).toBe(true);
		const token = await issued.text();

		await page.goto(`/setup?token=${encodeURIComponent(token)}`);
		await restoreFrom(page, backupPath);
		await expect(page).toHaveURL(/\/login$/);

		// 戻したデータの管理者でログインする
		await logInAsAdmin(page);
		expect(await contentTitles(page)).toContain(keptTitle);
	});
});
