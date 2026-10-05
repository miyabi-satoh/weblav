import { test as setup } from '@playwright/test';
import { ADMIN_USERNAME, ADMIN_PASSWORD, ADMIN_STORAGE_STATE } from './auth';
import { registerContentRoot } from './fixture-helpers';
import { logIn, PASSWORD_HASH_TIMEOUT } from './helpers';

setup('管理者としてログインする', async ({ page }) => {
	await logIn(page, ADMIN_USERNAME, ADMIN_PASSWORD);
	await page.waitForURL('/', { timeout: PASSWORD_HASH_TIMEOUT });
	await page.context().storageState({ path: ADMIN_STORAGE_STATE });
	// フィクスチャの置き場を「公開できるフォルダ」に入れておく。これが無いと
	// folder / archive を1つも登録できない (→ docs/folders.md「公開できるフォルダ」)。
	// 登録の口はループバック限定だが、e2e は 127.0.0.1 に当てているので条件を満たす。
	await registerContentRoot(page.request);
});
