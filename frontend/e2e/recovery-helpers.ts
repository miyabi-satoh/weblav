import { expect, type Page } from '@playwright/test';
import { RECOVERY_CODE_KEPT_BUTTON_NAME, RECOVERY_CODE_SAVE_BUTTON_NAME } from './helpers';

/**
 * 見せているリカバリコードを保存してから「保管しました」を押し、コードを返す。
 * コピーはヘッドレスのブラウザーではクリップボードの許可が要るので、保存で済ませる。
 */
export async function keepRecoveryCode(page: Page): Promise<string> {
	const code = (await page.locator('output').textContent())?.trim() ?? '';
	expect(code).toMatch(/^[0-9A-Z]{5}(-[0-9A-Z]{5}){3}$/);
	const kept = page.getByRole('button', { name: RECOVERY_CODE_KEPT_BUTTON_NAME });
	// コピー・保存・印刷のどれかをするまで押せない。
	await expect(kept).toBeDisabled();
	const download = page.waitForEvent('download');
	await page.getByRole('button', { name: RECOVERY_CODE_SAVE_BUTTON_NAME }).click();
	await download;
	await kept.click();
	return code;
}
