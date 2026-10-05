// LAN からのプレーン HTTP (セキュアコンテキストでない) で、ダイアログからコピーできるか。
// e2e は 127.0.0.1 に当てていてセキュアコンテキストになるので、`navigator.clipboard` を隠して
// execCommand の道を通す。写った中身は、隠す前に取っておいた本物で読む。
import { test, expect, type Browser, type Page } from '@playwright/test';
import { ADMIN_STORAGE_STATE, ADMIN_USERNAME } from './auth';
import {
	RECOVERY_CODE_CREATE_BUTTON_NAME,
	CURRENT_PASSWORD_LABEL,
	RECOVERY_CODE_KEPT_BUTTON_NAME,
	RECOVERY_CODE_MENU_ITEM_NAME
} from './helpers';

const RECOVERY_CODE_COPY_BUTTON_NAME = /^コピー$|^Copy$/;
const CONNECTION_INFO_TRIGGER_NAME = /^他の端末からつなぐ$|^Connect from another device$/;
const CONNECTION_INFO_COPY_BUTTON_NAME = /^コピーする$|^Copy$/;

type WindowWithClipboard = Window & { __realClipboard?: Clipboard };

async function insecurePage(browser: Browser, baseURL: string | undefined): Promise<Page> {
	const context = await browser.newContext({ storageState: ADMIN_STORAGE_STATE });
	await context.grantPermissions(['clipboard-read'], { origin: baseURL });
	await context.addInitScript(() => {
		(window as WindowWithClipboard).__realClipboard = navigator.clipboard;
		Object.defineProperty(Navigator.prototype, 'clipboard', { get: () => undefined });
	});
	return context.newPage();
}

function readClipboard(page: Page): Promise<string | undefined> {
	return page.evaluate(() => (window as WindowWithClipboard).__realClipboard?.readText());
}

test('管理者のメニューから作ったリカバリコードをコピーできる', async ({ browser, baseURL }) => {
	const page = await insecurePage(browser, baseURL);
	try {
		// 作り直すと共有の管理者のコードが変わるので、作る API は横取りする。
		const fakeCode = 'AAAAA-BBBBB-CCCCC-DDDDD';
		await page.route('**/api/v1/auth/recovery-code', (route) =>
			route.fulfill({ json: { recoveryCode: fakeCode } })
		);
		await page.goto('/');
		await page.getByRole('button', { name: ADMIN_USERNAME, exact: true }).click();
		await page.getByRole('menuitem', { name: RECOVERY_CODE_MENU_ITEM_NAME }).click();
		const dialog = page.getByRole('dialog');
		await dialog.getByLabel(CURRENT_PASSWORD_LABEL).fill('unused');
		await dialog.getByRole('button', { name: RECOVERY_CODE_CREATE_BUTTON_NAME }).click();
		await expect(dialog.getByText(fakeCode)).toBeVisible();

		const copyButton = dialog.getByRole('button', { name: RECOVERY_CODE_COPY_BUTTON_NAME });
		await copyButton.click();
		await expect(copyButton).toBeFocused();
		await expect(
			dialog.getByRole('button', { name: RECOVERY_CODE_KEPT_BUTTON_NAME })
		).toBeEnabled();
		expect(await readClipboard(page)).toBe(fakeCode);
	} finally {
		await page.context().close();
	}
});

test('「他の端末からつなぐ」のアドレスをコピーできる', async ({ browser, baseURL }) => {
	const page = await insecurePage(browser, baseURL);
	try {
		// mDNS が使えない環境でもアドレスを出すよう、ホスト名を返させる。
		await page.route('**/api/v1/connection-info', (route) =>
			route.fulfill({ json: { mdnsHostname: 'e2e-host.local', port: 3000 } })
		);
		await page.goto('/');
		await page.getByRole('button', { name: CONNECTION_INFO_TRIGGER_NAME }).click();
		const dialog = page.getByRole('dialog');
		const copyButton = dialog.getByRole('button', { name: CONNECTION_INFO_COPY_BUTTON_NAME });
		await copyButton.click();
		await expect(copyButton).toBeFocused();
		expect(await readClipboard(page)).toBe(`${new URL(baseURL!).protocol}//e2e-host.local:3000`);
	} finally {
		await page.context().close();
	}
});
