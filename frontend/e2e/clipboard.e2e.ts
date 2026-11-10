// LAN からのプレーン HTTP (セキュアコンテキストでない) で、ダイアログからコピーできるか。
// e2e は 127.0.0.1 に当てていてセキュアコンテキストになるので、`navigator.clipboard` を隠して
// execCommand の道を通す。写った中身は、隠す前に取っておいた本物で読む。
import { test, expect, type Browser, type Page } from '@playwright/test';
import { ADMIN_STORAGE_STATE } from './auth';
import { send } from './api-helpers';
import {
	RECOVERY_CODE_CREATE_BUTTON_NAME,
	RECOVERY_CODE_KEPT_BUTTON_NAME,
	RECOVERY_CODE_REPLACE_BUTTON_NAME
} from './helpers';

const COPY_BUTTON_NAME = /^コピー$|^Copy$/;
const RECOVERY_CODE_ACTION_NAME = new RegExp(
	`${RECOVERY_CODE_CREATE_BUTTON_NAME.source}|${RECOVERY_CODE_REPLACE_BUTTON_NAME.source}`
);
const REPLACE_CONFIRM_NAME = /^作り直す$|^Regenerate$/;
const SHARE_TRIGGER_NAME = /^このページを共有$|^Share this page$/;
const SHARE_QR_MENU_ITEM_NAME = /^QR コードを表示\.\.\.$|^Show QR code\.\.\.$/;
const SHARE_ADDRESS_SELECT_NAME = /^共有するアドレス$|^Address to share$/;

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

test('アカウントのページで作ったリカバリコードをコピーできる', async ({ browser, baseURL }) => {
	const page = await insecurePage(browser, baseURL);
	try {
		// 作り直すと共有の管理者のコードが変わるので、作る API は横取りする。
		const fakeCode = 'AAAAA-BBBBB-CCCCC-DDDDD';
		await page.route('**/api/v1/auth/recovery-code', (route) =>
			route.fulfill({ json: { recoveryCode: fakeCode } })
		);
		await page.goto('/account');
		// 共有の管理者がもうコードを持っていれば、作り直しの確かめを挟む。
		const me = (await send(page.request, 'get', '/auth/me')) as { hasRecoveryCode: boolean };
		await page.getByRole('button', { name: RECOVERY_CODE_ACTION_NAME }).click();
		if (me.hasRecoveryCode) {
			await page
				.getByRole('alertdialog')
				.getByRole('button', { name: REPLACE_CONFIRM_NAME })
				.click();
		}
		const dialog = page.getByRole('dialog');
		await expect(dialog.getByText(fakeCode)).toBeVisible();

		const copyButton = dialog.getByRole('button', { name: COPY_BUTTON_NAME });
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

test('共有の QR コードの画面から、ページのアドレスをコピーできる', async ({ browser, baseURL }) => {
	const page = await insecurePage(browser, baseURL);
	try {
		// mDNS が使えない環境でもアドレスを出すよう、ホスト名を返させる。
		await page.route('**/api/v1/connection-info', (route) =>
			route.fulfill({ json: { mdnsHostname: 'e2e-host.local', port: 8370, lanAddresses: [] } })
		);
		await page.goto('/');
		await page.getByRole('button', { name: SHARE_TRIGGER_NAME }).click();
		await page.getByRole('menuitem', { name: SHARE_QR_MENU_ITEM_NAME }).click();
		const dialog = page.getByRole('dialog');
		const copyButton = dialog.getByRole('button', { name: COPY_BUTTON_NAME });
		await copyButton.click();
		await expect(copyButton).toBeFocused();
		expect(await readClipboard(page)).toBe(`${new URL(baseURL!).protocol}//e2e-host.local:8370/`);
	} finally {
		await page.context().close();
	}
});

test('共有の QR コードの画面で IP アドレスを選ぶと、そのアドレスをコピーする', async ({
	browser,
	baseURL
}) => {
	const page = await insecurePage(browser, baseURL);
	try {
		await page.route('**/api/v1/connection-info', (route) =>
			route.fulfill({
				json: { mdnsHostname: 'e2e-host.local', port: 8370, lanAddresses: ['192.168.1.13'] }
			})
		);
		await page.goto('/');
		await page.getByRole('button', { name: SHARE_TRIGGER_NAME }).click();
		await page.getByRole('menuitem', { name: SHARE_QR_MENU_ITEM_NAME }).click();
		const dialog = page.getByRole('dialog');
		await dialog.getByRole('button', { name: SHARE_ADDRESS_SELECT_NAME }).click();
		await page.getByRole('option', { name: '192.168.1.13:8370' }).click();
		await dialog.getByRole('button', { name: COPY_BUTTON_NAME }).click();
		await expect
			.poll(() => readClipboard(page))
			.toBe(`${new URL(baseURL!).protocol}//192.168.1.13:8370/`);
	} finally {
		await page.context().close();
	}
});
