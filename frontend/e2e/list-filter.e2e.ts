// 一覧のページ内の絞り込み (管理者ログイン済み)。当たり方の決まりは src/lib/list-filter.spec.ts で見るので、
// ここでは打つと行が減り、当たった文字が強調され、語が URL に載り、戻ったときに残ることだけを確かめる。
import { test, expect } from '@playwright/test';
import { setUpArchiveForBrowsing, withDirectoryContent } from './fixture-helpers';

test.use({ actionTimeout: 10_000, navigationTimeout: 10_000 });

test('一覧の絞り込み: 打つと当たる行だけが残り、語が URL に載る', async ({ page }) => {
	await withDirectoryContent(
		page.request,
		{ type: 'archive', namePrefix: 'e2e-list-filter' },
		async (archive) => {
			await setUpArchiveForBrowsing(page.request, archive);

			await page.goto(`/archives/${archive.id}`);
			const rows = page.getByRole('button', { name: /リスニング/ });
			await expect(rows).toHaveCount(2);

			// 間を空けた当たり (「2023 リスニング」の 2・3・リ)。
			const input = page.getByRole('textbox', { name: /^(名前で絞り込み|Filter by name)$/ });
			await input.fill('23リ');
			await expect(rows).toHaveCount(1);
			await expect(rows.first()).toContainText('2023');
			await expect(rows.first().locator('mark')).not.toHaveCount(0);
			await expect(page).toHaveURL(/[?&]filter=23%E3%83%AA/);

			// 別の画面へ行って戻っても、語と絞った行が残る (shallow routing の語は page.state から戻る)。
			await page.getByRole('link', { name: 'WebLAV', exact: true }).click();
			await page.waitForURL((url) => url.pathname === '/');
			await page.goBack();
			await expect(input).toHaveValue('23リ');
			await expect(rows).toHaveCount(1);
		}
	);
});
