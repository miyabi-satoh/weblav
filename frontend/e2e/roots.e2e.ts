// 「公開できるフォルダー」の管理画面 (→ docs/folders.md「公開できるフォルダー」)。
// サーバーの PC からしか触れない画面なので、別のマシンのサーバーに当てるときは飛ばす。
// ファイルシステムの実体が要るため、admin.e2e.ts ではなくこちらに置く。
import { realpathSync } from 'node:fs';
import path from 'node:path';
import { test, expect, type Page, type Route } from '@playwright/test';
import { createRootByApi, removeRootByApi, send, withContentByApi } from './api-helpers';
import {
	createRootFixtureTree,
	FIXTURE_UNREACHABLE,
	removeFixtureTree,
	sharedContentRoot
} from './fixture-helpers';
import {
	clickRowMenuItem,
	confirmDelete,
	DELETE_MENU_ITEM_NAME,
	RENAME_MENU_ITEM_NAME,
	ROOT_ADD_BUTTON_NAME,
	ROOT_PICKING_TEXT,
	SAVE_BUTTON_NAME,
	uniqueId
} from './helpers';
import { SERVER_IS_REMOTE, SERVER_IS_REMOTE_REASON } from './server-location';

test.skip(SERVER_IS_REMOTE, SERVER_IS_REMOTE_REASON);

test('登録したフォルダーが並び、画面から削除できる', async ({ page }) => {
	const root = createRootFixtureTree(uniqueId('roots'));
	let id: number | null = null;
	try {
		id = await createRootByApi(page.request, root);
		test.skip(id === null, FIXTURE_UNREACHABLE);

		await page.goto('/admin/roots');
		const row = page.getByRole('listitem').filter({ hasText: root });
		await expect(row).toBeVisible();

		await clickRowMenuItem(page, row, DELETE_MENU_ITEM_NAME);
		await confirmDelete(page);

		await expect(page.getByRole('listitem').filter({ hasText: root })).toHaveCount(0);
	} finally {
		removeFixtureTree(root);
		// 画面での削除まで届かずに失敗したときのため。既に消えていれば 404 で、そのままでよい。
		if (id !== null) await removeRootByApi(page.request, id);
	}
});

/**
 * OS のフォルダー選択の窓 (→ docs/folders.md「選び方」) の代わりに、窓を開く口の応答を差し替える。
 * 窓はブラウザーの外に出て自動では操作できないので、サーバーには届かせない。
 */
async function stubFolderPicker(page: Page, respond: (route: Route) => Promise<void>) {
	await page.route('**/api/v1/admin/roots/pick', respond);
}

// 窓で選ぶとすぐフォルダー名で追加する。一覧には名前とフルパスの両方が出る (→ docs/folders.md「公開できるフォルダー」)。
test('フォルダーを選ぶとフォルダー名で追加されて名前とフルパスが並び、名前を変えられる', async ({
	page
}) => {
	const root = createRootFixtureTree(uniqueId('roots-named'));
	const renamed = uniqueId('付け直し');
	try {
		const target = realpathSync(root);
		await stubFolderPicker(page, (route) => route.fulfill({ json: { path: target } }));
		await page.goto('/admin/roots');
		await page.getByRole('button', { name: ROOT_ADD_BUTTON_NAME }).click();

		// 名前は入力させない。フォルダー名で追加される。
		const name = path.basename(target);
		const row = page.getByRole('listitem').filter({ hasText: target });
		await expect(row.getByText(name, { exact: true })).toBeVisible();

		// 行のメニューから名前を変えられる。
		await clickRowMenuItem(page, row, RENAME_MENU_ITEM_NAME);
		const rename = page.getByRole('dialog', { name: /^名前を変更$|^Rename$/ });
		const renameInput = rename.getByLabel(/^\s*(名前|Name)\s*$/);
		await expect(renameInput).toHaveValue(name);
		await renameInput.fill(renamed);
		await rename.getByRole('button', { name: SAVE_BUTTON_NAME }).click();
		await expect(rename).toHaveCount(0);
		await expect(
			page.getByRole('listitem').filter({ hasText: renamed }).getByText(target, { exact: true })
		).toBeVisible();
	} finally {
		const roots = (await send(page.request, 'get', '/admin/roots')) as {
			id: number;
			name: string;
		}[];
		const added = roots.find(
			(entry) => entry.name === path.basename(root) || entry.name === renamed
		);
		if (added !== undefined) await removeRootByApi(page.request, added.id);
		removeFixtureTree(root);
	}
});

test('窓を閉じるまで案内を出し、キャンセルなら何も追加しない', async ({ page }) => {
	let close: () => void = () => {};
	const closed = new Promise<void>((resolve) => (close = resolve));
	await stubFolderPicker(page, async (route) => {
		await closed;
		await route.fulfill({ json: { path: null } });
	});
	await page.goto('/admin/roots');
	// 一覧が出てから数える。
	await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
	const rows = page.getByRole('listitem');
	const before = await rows.count();

	await page.getByRole('button', { name: ROOT_ADD_BUTTON_NAME }).click();
	await expect(page.getByText(ROOT_PICKING_TEXT)).toBeVisible();

	close();
	await expect(page.getByText(ROOT_PICKING_TEXT)).toHaveCount(0);
	await expect(page.getByRole('button', { name: ROOT_ADD_BUTTON_NAME })).toBeEnabled();
	await expect(rows).toHaveCount(before);
	await expect(page.getByRole('alertdialog')).toHaveCount(0);
});

// 窓ではどのフォルダーでも選べるので、断られたら理由を出す (→ docs/folders.md「選び方」)。
test('登録済みのフォルダーを選ぶと、登録済みだと伝える', async ({ page }) => {
	const root = createRootFixtureTree(uniqueId('roots-twice'));
	let id: number | null = null;
	try {
		id = await createRootByApi(page.request, root);
		test.skip(id === null, FIXTURE_UNREACHABLE);
		const target = realpathSync(root);
		await stubFolderPicker(page, (route) => route.fulfill({ json: { path: target } }));
		await page.goto('/admin/roots');
		await page.getByRole('button', { name: ROOT_ADD_BUTTON_NAME }).click();

		await expect(page.getByRole('alertdialog')).toContainText(
			new RegExp(
				`「${path.basename(target)}」として登録済み|already registered as "${path.basename(target)}"`
			)
		);
	} finally {
		if (id !== null) await removeRootByApi(page.request, id);
		removeFixtureTree(root);
	}
});

test('窓がすでに開いているときは、そう伝える', async ({ page }) => {
	await stubFolderPicker(page, (route) =>
		route.fulfill({
			status: 409,
			json: { error: { code: 'conflict', message: 'a folder picker is already open' } }
		})
	);
	await page.goto('/admin/roots');
	await page.getByRole('button', { name: ROOT_ADD_BUTTON_NAME }).click();

	await expect(
		page.getByText(
			/^フォルダーを選ぶ窓が、すでに開いています。|^A window for choosing a folder is already open\./
		)
	).toBeVisible();
});

test('管理画面のタブから開ける', async ({ page }) => {
	await page.goto('/admin/contents');
	await page
		.getByRole('navigation')
		.getByRole('link', { name: /^公開できるフォルダー$|^Shared folders$/ })
		.click();

	await expect(page.getByRole('heading', { level: 1 })).toHaveText(
		/^公開できるフォルダー$|^Shared folders$/
	);
});

// 登録を消しても、そのフォルダーを指すコンテンツの登録は残る。その編集ページのピッカーが、
// 辿れない位置で行き止まりにならず、上位の一覧から選び直せること (→ docs/folders.md「公開できるフォルダー」)。
test('登録を消したフォルダーを指すコンテンツでも、ピッカーは上位の一覧から開き直せる', async ({
	page
}) => {
	const root = createRootFixtureTree(uniqueId('roots-gone'));
	let rootId: number | null = null;
	try {
		rootId = await createRootByApi(page.request, root);
		test.skip(rootId === null, FIXTURE_UNREACHABLE);

		await withContentByApi(
			page.request,
			{
				type: 'folder',
				title: uniqueId('gone-folder'),
				path: root,
				visibility: 'hidden'
			},
			async (content) => {
				await send(page.request, 'delete', `/admin/roots/${rootId}`);
				rootId = null;

				await page.goto(`/admin/contents/${content.id}`);
				await page.getByRole('button', { name: /^選び直す\.\.\.$|^Change\.\.\.$/ }).click();

				// 上位の一覧 (登録済みの「公開できるフォルダー」。名前で並ぶ) に落ちていること。
				// 開発用 DB では置き場の登録が別の名前・親のフォルダーのこともあるので、実際の名前を引く。
				const shared = await sharedContentRoot(page.request);
				test.skip(shared === null, FIXTURE_UNREACHABLE);
				await expect(page.getByText(shared!.rootName, { exact: true })).toBeVisible();
			}
		);
	} finally {
		if (rootId !== null) await removeRootByApi(page.request, rootId);
		removeFixtureTree(root);
	}
});
