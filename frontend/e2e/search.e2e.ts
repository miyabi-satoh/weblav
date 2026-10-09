// 検索の画面 (管理者ログイン済み)。一致の決まりと見える範囲はサーバーの単体テストで見るので、
// ここではヘッダーから開いて打つと結果が出て、押すと元の一覧と同じように開くことだけを確かめる。
import { test, expect } from '@playwright/test';
import { AUDIO_PLAYER_REGION_NAME } from './helpers';
import { setUpArchiveForBrowsing, withDirectoryContent } from './fixture-helpers';

test.use({ actionTimeout: 10_000, navigationTimeout: 10_000 });

const SEARCH_LINK_NAME = /^検索$|^Search$/;
// ラベルは文言の前後で改行しているので、前後の空白を許す。
const SEARCH_INPUT_LABEL = /^\s*(探す語|Search for)\s*$/;

test('検索: ヘッダーから開いて打つと、アーカイブのファイルが出て、押すとその場で鳴る', async ({
	page
}) => {
	await withDirectoryContent(
		page.request,
		{ type: 'archive', namePrefix: 'e2e-search' },
		async (archive, name) => {
			await setUpArchiveForBrowsing(page.request, archive);

			await page.goto('/');
			await page.getByRole('link', { name: SEARCH_LINK_NAME }).click();
			const input = page.getByLabel(SEARCH_INPUT_LABEL);
			await expect(input).toBeFocused();
			await input.fill('2023 リスニング');
			await page.waitForURL((url) => url.searchParams.get('q') === '2023 リスニング');

			// ほかのテストのアーカイブも当たりうるので、2段目のアーカイブの名前で行を選ぶ。
			const row = page.getByRole('button', { name: /2023 リスニング/ }).filter({ hasText: name });
			await row.click();
			await expect(page.getByRole('region', { name: AUDIO_PLAYER_REGION_NAME })).toBeVisible();
			await expect(page.locator('audio')).toHaveAttribute(
				'src',
				new RegExp(`/api/v1/contents/${archive.id}/items/\\d+/download$`)
			);
		}
	);
});

test('検索: 閲覧ページから開くとその中を探し、「すべて」で全体に切り替わる', async ({ page }) => {
	await withDirectoryContent(
		page.request,
		{ type: 'archive', namePrefix: 'e2e-search-scope' },
		async (archive, name) => {
			await setUpArchiveForBrowsing(page.request, archive);

			await page.goto(`/archives/${archive.id}`);
			await page.getByRole('link', { name: SEARCH_LINK_NAME }).click();
			const scope = page.getByRole('group', { name: /^(探す範囲|Where to search)$/ });
			const within = scope.getByRole('radio', { name: new RegExp(name) });
			await expect(within).toBeChecked();
			await page.getByLabel(SEARCH_INPUT_LABEL).fill('2023 リスニング');
			await page.waitForURL((url) => url.searchParams.get('q') === '2023 リスニング');
			await expect(page).toHaveURL(new RegExp(`within=${archive.id}`));

			// 範囲の中の行は、どれもこのアーカイブのもの。
			const rows = page.getByRole('button', { name: /2023 リスニング/ });
			await expect(rows.first()).toBeVisible();
			await expect(rows.filter({ hasNotText: name })).toHaveCount(0);

			await scope.getByRole('radio', { name: /^(すべて|Everywhere)$/ }).click();
			await page.waitForURL((url) => url.searchParams.get('all') === '1');
			await expect(within).not.toBeChecked();
		}
	);
});
