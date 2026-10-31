// 「共有」の管理画面 (→ docs/folders.md「共有」)。
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
	CHANGE_PATH_BUTTON_NAME,
	clickRowMenuItem,
	confirmDelete,
	DELETE_MENU_ITEM_NAME,
	DISCARD_AND_LEAVE_BUTTON_NAME,
	LEAVE_CONFIRM_NAME,
	ROOT_ADD_BUTTON_LOADING_NAME,
	ROOT_ADD_BUTTON_NAME,
	SAVE_BUTTON_NAME,
	saveForm,
	uniqueId
} from './helpers';
import { SERVER_IS_REMOTE, SERVER_IS_REMOTE_REASON } from './server-location';

test.skip(SERVER_IS_REMOTE, SERVER_IS_REMOTE_REASON);

test('登録した共有が並び、画面から削除できる', async ({ page }) => {
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

/** 共有の編集ページの、区画のフォーム (見出しで名前が付く)。 */
function sectionForm(page: Page, name: RegExp) {
	return page.getByRole('form', { name });
}
const NAME_SECTION_NAME = /^名前$|^Name$/;
const ACCESS_SECTION_NAME = /^権限$|^Permissions$/;
const REPICK_BUTTON_NAME = /^フォルダーを選び直す\.\.\.$|^Choose folder again\.\.\.$/;
const EDIT_MENU_ITEM_NAME = /^編集$|^Edit$/;

// 窓で選ぶとすぐフォルダー名で追加し、その共有の編集ページへ移る (→ docs/ui.md「UI 全般」の管理画面の共有)。
test('フォルダーを選ぶとフォルダー名で追加されて編集ページへ移り、名前と権限を区画ごとに保存できる', async ({
	page
}) => {
	const root = createRootFixtureTree(uniqueId('roots-named'));
	const renamed = uniqueId('付け直し');
	try {
		const target = realpathSync(root);
		await stubFolderPicker(page, (route) => route.fulfill({ json: { path: target } }));
		await page.goto('/admin/roots');
		await page.getByRole('button', { name: ROOT_ADD_BUTTON_NAME }).click();

		// 名前は入力させない。フォルダー名で追加され、場所とともに編集ページに出る。
		await page.waitForURL(/\/admin\/roots\/\d+$/);
		const name = path.basename(target);
		await expect(page.getByRole('heading', { level: 1 })).toHaveText(name);
		await expect(page.getByTestId('root-path')).toHaveText(target);

		// 変えるまで保存できない。
		const nameForm = sectionForm(page, NAME_SECTION_NAME);
		const nameSave = nameForm.getByRole('button', { name: SAVE_BUTTON_NAME });
		await expect(nameSave).toBeDisabled();
		await nameForm.getByRole('textbox').fill(renamed);
		await saveForm(page, {
			button: nameSave,
			toastText: /「名前」を保存しました。|“Name” saved\./
		});
		await expect(page.getByRole('heading', { level: 1 })).toHaveText(renamed);

		const accessForm = sectionForm(page, ACCESS_SECTION_NAME);
		await accessForm
			.getByRole('button', { name: /^一般ユーザーの既定$|^Default for standard users$/ })
			.click();
		await page.getByRole('option', { name: /^足すだけ$|^Add only$/ }).click();
		await saveForm(page, {
			button: accessForm.getByRole('button', { name: SAVE_BUTTON_NAME }),
			toastText: /「権限」を保存しました。|“Permissions” saved\./
		});

		// 一覧に戻ると、行に変えた名前と権限が出て、行を押すと編集ページが開く。
		await page
			.getByRole('link', { name: /^共有$|^Shares$/ })
			.first()
			.click();
		await page.waitForURL(/\/admin\/roots$/);
		const row = page.getByRole('listitem').filter({ hasText: target });
		await expect(row.getByText(renamed, { exact: true })).toBeVisible();
		await expect(row).toContainText(/一般ユーザー: 足すだけ|Standard users: Add only/);
		// 名前のリンクではなく行の真ん中を押す (リンクの押せる範囲を行いっぱいに広げてある)。
		await row.click();
		await page.waitForURL(/\/admin\/roots\/\d+$/);
		await expect(page.getByRole('heading', { level: 1 })).toHaveText(renamed);
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

test('編集ページ: 保存していない変更があると、離れる前に確かめる', async ({ page }) => {
	const root = createRootFixtureTree(uniqueId('roots-leave'));
	let id: number | null = null;
	try {
		id = await createRootByApi(page.request, root);
		test.skip(id === null, FIXTURE_UNREACHABLE);
		await page.goto('/admin/roots');
		await clickRowMenuItem(
			page,
			page.getByRole('listitem').filter({ hasText: realpathSync(root) }),
			EDIT_MENU_ITEM_NAME
		);
		await page.waitForURL(/\/admin\/roots\/\d+$/);

		await sectionForm(page, NAME_SECTION_NAME).getByRole('textbox').fill(uniqueId('書きかけ'));
		const back = page.getByRole('link', { name: /^共有$|^Shares$/ }).first();
		await back.click();
		const confirm = page.getByRole('alertdialog', { name: LEAVE_CONFIRM_NAME });
		await expect(confirm).toBeVisible();
		await confirm.getByRole('button', { name: /^キャンセル$|^Cancel$/ }).click();
		await expect(page).toHaveURL(/\/admin\/roots\/\d+$/);

		await back.click();
		await confirm.getByRole('button', { name: DISCARD_AND_LEAVE_BUTTON_NAME }).click();
		await page.waitForURL(/\/admin\/roots$/);
	} finally {
		if (id !== null) await removeRootByApi(page.request, id);
		removeFixtureTree(root);
	}
});

// フォルダーを移した先を選び直すと、中のフォルダーのコンテンツも新しい場所を開く (→ docs/shares.md「共有の場所の選び直し」)。
test('編集ページ: 場所を選び直すと、中のコンテンツも新しい場所へ移る', async ({ page }) => {
	const before = createRootFixtureTree(uniqueId('roots-move-from'));
	const after = createRootFixtureTree(uniqueId('roots-move-to'));
	let id: number | null = null;
	try {
		id = await createRootByApi(page.request, before);
		test.skip(id === null, FIXTURE_UNREACHABLE);
		const target = realpathSync(after);
		await withContentByApi(
			page.request,
			{
				type: 'folder',
				title: uniqueId('moving-folder'),
				path: path.join(realpathSync(before), '2024'),
				visibility: 'hidden'
			},
			async (content) => {
				await stubFolderPicker(page, (route) => route.fulfill({ json: { path: target } }));
				await page.goto(`/admin/roots/${id}`);
				await page.getByRole('button', { name: REPICK_BUTTON_NAME }).click();

				const confirm = page.getByRole('alertdialog', {
					name: /^共有の場所を変更しますか？$|^Change the location of this share\?$/
				});
				await expect(confirm).toContainText(target);
				await confirm.getByRole('button', { name: /^変更する$|^Change$/ }).click();
				await expect(page.getByTestId('root-path')).toHaveText(target);

				// 新しい場所の同じフォルダーを開く。
				const contents = (await send(page.request, 'get', '/admin/contents')) as {
					id: number;
					path: string | null;
				}[];
				expect(contents.find((entry) => entry.id === content.id)?.path).toBe(
					path.join(target, '2024')
				);
				await page.goto(`/folders/${content.id}`);
				await expect(page.getByText('listening.mp3')).toBeVisible();
			}
		);
	} finally {
		if (id !== null) await removeRootByApi(page.request, id);
		removeFixtureTree(before);
		removeFixtureTree(after);
	}
});

test('窓を閉じるまでボタンを押せなくし、キャンセルなら何も追加しない', async ({ page }) => {
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
	await expect(page.getByRole('button', { name: ROOT_ADD_BUTTON_LOADING_NAME })).toBeDisabled();

	close();
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
				`「${path.basename(target)}」として登録済み|already registered as “${path.basename(target)}”`
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
			/^フォルダーを選ぶウィンドウがすでに開いています。|^A window for choosing a folder is already open\./
		)
	).toBeVisible();
});

test('管理画面のタブから開ける', async ({ page }) => {
	await page.goto('/admin/contents');
	await page
		.getByRole('navigation')
		.getByRole('link', { name: /^共有$|^Shares$/ })
		.click();

	await expect(page.getByRole('heading', { level: 1 })).toHaveText(/^共有$|^Shares$/);
});

// 登録を消しても、そのフォルダーを指すコンテンツの登録は残る。その編集ページのピッカーが、
// 辿れない位置で行き止まりにならず、上位の一覧から選び直せること (→ docs/folders.md「共有」)。
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
				await page.getByRole('button', { name: CHANGE_PATH_BUTTON_NAME }).click();

				// 上位の一覧 (登録済みの「共有」。名前で並ぶ) に落ちていること。
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
