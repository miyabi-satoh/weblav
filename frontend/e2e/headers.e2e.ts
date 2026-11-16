// 全ての応答に付ける CSP (→ docs/architecture.md「応答のヘッダー」) が、画面の中で使う枠を止めないこと。
// 枠の中の文書は親の CSP を引き継ぐので (srcdoc)、ヘッダーを変えたときに印刷と動画の埋め込みが黙って壊れうる。
import { test, expect, type Page } from '@playwright/test';
import { createUserByApi, withContentByApi } from './api-helpers';
import {
	ACCOUNT_MENU_ITEM_NAME,
	RECOVERY_CODE_CREATE_BUTTON_NAME,
	RECOVERY_CODE_KEPT_BUTTON_NAME,
	anonymousContext,
	logInToHome,
	uniqueId
} from './helpers';
import { removeUserIfPresent } from './user-helpers';

const PRINT_BUTTON_NAME = /^印刷$|^Print$/;
const RECOVERY_SECTION_NAME = /^リカバリコード$|^Recovery code$/;

/** ページと枠の中で出た CSP の違反を集める。 */
function collectCspViolations(page: Page): string[] {
	const violations: string[] = [];
	page.on('console', (message) => {
		if (/Content Security Policy/i.test(message.text())) violations.push(message.text());
	});
	return violations;
}

test('リカバリコードの印刷: 文面だけの枠が描かれ、CSP に止められない', async ({
	page,
	browser
}) => {
	test.slow();
	const username = uniqueId('e2e-print');
	await createUserByApi(page.request, username, 'password', 'user');

	const context = await anonymousContext(browser);
	// 印刷の窓はヘッドレスのブラウザーでは確かめられないので、枠の中の print を、文面を残して閉じたことにする形に替える。
	// 初期化のスクリプトは枠の中の文書にも入る。
	await context.addInitScript(() => {
		if (window.top === window) return;
		window.print = () => {
			(window.top as Window & { __printed?: string }).__printed = document.body.innerText;
			window.dispatchEvent(new Event('afterprint'));
		};
	});
	const own = await context.newPage();
	const violations = collectCspViolations(own);
	try {
		await logInToHome(own, username, 'password');
		await own.getByRole('button', { name: username, exact: true }).click();
		await own.getByRole('menuitem', { name: ACCOUNT_MENU_ITEM_NAME }).click();
		await own.waitForURL('/account');
		const section = own.getByRole('region', { name: RECOVERY_SECTION_NAME });
		await section.getByRole('button', { name: RECOVERY_CODE_CREATE_BUTTON_NAME }).click();
		const code = (await own.locator('output').textContent())?.trim() ?? '';
		expect(code).toMatch(/^[0-9A-Z]{5}(-[0-9A-Z]{5}){3}$/);

		await own.getByRole('button', { name: PRINT_BUTTON_NAME }).click();
		await expect
			.poll(() => own.evaluate(() => (window as Window & { __printed?: string }).__printed))
			.toContain(code);
		await expect(own.getByRole('button', { name: RECOVERY_CODE_KEPT_BUTTON_NAME })).toBeEnabled();
		expect(violations).toEqual([]);
	} finally {
		await context.close();
		await removeUserIfPresent(page, username);
	}
});

test('動画サイトの埋め込み: ビューアーの枠に YouTube のプレイヤーを読み込み、CSP に止められない', async ({
	page
}) => {
	// 外へはつながず、埋め込みの口への要求をこちらで答える。
	await page.route('https://www.youtube-nocookie.com/**', (route) =>
		route.fulfill({
			contentType: 'text/html',
			body: '<!doctype html><title>player</title><p>e2e player</p>'
		})
	);
	const violations = collectCspViolations(page);
	const title = uniqueId('E2E動画');
	await withContentByApi(
		page.request,
		{ type: 'link', title, url: 'https://www.youtube.com/watch?v=dQw4w9WgXcQ' },
		async () => {
			await page.goto('/');
			await page.getByRole('link', { name: title, exact: true }).click();
			const player = page.frameLocator('iframe[src^="https://www.youtube-nocookie.com/embed/"]');
			await expect(player.getByText('e2e player')).toBeVisible();
			expect(violations).toEqual([]);
		}
	);
});
