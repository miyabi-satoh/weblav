// コンテンツを画面から登録する流れのうち、種別ごとに入力が違うもの (ファイル・フォルダー・アーカイブ)。
// リンクとグループは admin.e2e.ts で通している。
//
// フォルダーとアーカイブは、パスをテキストで入れさせずに選ばせ、登録の前に対象の件数を見せる
// (→ docs/folders.md「一覧 API」・「登録前の確認」)。そこまでを画面で辿る。
import path from 'node:path';
import { test, expect, type Page } from '@playwright/test';
import { send } from './api-helpers';
import {
	expectContentEditPage,
	openCreateForm,
	removeContentIfPresent,
	submitCreateForm
} from './content-helpers';
import {
	createFixtureTree,
	FIXTURE_UNREACHABLE,
	LOCATION_SEPARATOR,
	removeFixtureTree,
	sharedContentRoot,
	sharedLocationLabel,
	type SharedContentRoot
} from './fixture-helpers';
import {
	CHOOSE_PATH_BUTTON_NAME,
	CONFIRM_REGISTER_NAME,
	CONTENT_ADD_BUTTON_NAME,
	CONTENT_SUBMIT_BUTTON_NAME,
	CONTENT_TYPE_NAME,
	DIR_PICKER_NAME,
	openAddDialog,
	REGISTER_BUTTON_NAME,
	requiredLabel,
	TITLE_LABEL,
	uniqueId,
	USE_THIS_FOLDER_BUTTON_NAME
} from './helpers';

// 待ち続けて制限時間を使い切ると、後片付けまで時間切れになる (→ archive.e2e.ts)。
test.use({ actionTimeout: 10_000, navigationTimeout: 10_000 });

/**
 * パスを選ぶダイアログで、上位の一覧から置き場の中の `dirName` まで辿って決める。
 * 上位の一覧 (登録済みのフォルダーが並ぶだけの位置) では決められないことも確かめる。
 */
async function pickDirectory(page: Page, shared: SharedContentRoot, dirName: string) {
	await page.getByRole('button', { name: CHOOSE_PATH_BUTTON_NAME }).click();
	const picker = page.getByRole('dialog', { name: DIR_PICKER_NAME });
	const useThisFolder = picker.getByRole('button', { name: USE_THIS_FOLDER_BUTTON_NAME });

	// 上位の一覧には、フルパスではなく登録した名前が並ぶ (→ docs/folders.md「公開できるフォルダー」)。
	const rootButton = picker.getByRole('button', { name: shared.rootName, exact: true });
	await expect(rootButton).toBeVisible();
	await expect(useThisFolder).toBeDisabled();

	// 一覧は押すたびに読み直すので、「選択中」が移り終わるのを待ってから次を押す。
	// 待たずに決めると、読み込み中の1つ上の階層 (これも選べる) を決めてしまう。
	// 「選択中」は「名前 / その先」の形で出る。公開できるフォルダーそのものではパンくずの末尾と
	// 同じ文字列になるので、後ろ (選択中) を見る。
	const labels = [shared.rootName];
	const selected = () => picker.getByText(labels.join(LOCATION_SEPARATOR), { exact: true }).last();
	await rootButton.click();
	await expect(selected()).toBeVisible();
	const target = path.join(shared.contentRoot, dirName);
	for (const segment of path.relative(shared.root, target).split(path.sep)) {
		await picker.getByRole('button', { name: segment, exact: true }).click();
		labels.push(segment);
		await expect(selected()).toBeVisible();
	}
	await expect(useThisFolder).toBeEnabled();
	await useThisFolder.click();
	await expect(picker).toHaveCount(0);
}

/**
 * 登録前の確認で件数と閲覧範囲の説明を確かめ、登録して、移った先の編集ページの見出しを見る。
 */
async function confirmRegister(page: Page, title: string, scopeText: RegExp) {
	await page.getByRole('button', { name: CONTENT_SUBMIT_BUTTON_NAME }).click();
	const confirm = page.getByRole('dialog', { name: CONFIRM_REGISTER_NAME });
	// フィクスチャの木には3ファイルある (→ fixture-helpers.ts)。
	await expect(confirm.getByText(scopeText)).toBeVisible();
	await confirm.getByRole('button', { name: REGISTER_BUTTON_NAME }).click();
	await expectContentEditPage(page, title);
}

test('フォルダー: パスを辿って選び、件数と閲覧範囲を確かめてから登録できる', async ({ page }) => {
	const shared = await sharedContentRoot(page.request);
	test.skip(shared === null, FIXTURE_UNREACHABLE);

	const name = uniqueId('e2e-register-folder');
	const tree = createFixtureTree(name);
	try {
		await openCreateForm(page, 'folder');
		// パスを選ぶまでは追加できない。
		await expect(page.getByRole('button', { name: CONTENT_SUBMIT_BUTTON_NAME })).toBeDisabled();

		await pickDirectory(page, shared!, name);
		// 選んだ場所は、フルパスではなく「公開できるフォルダーの名前 / その先」で出る。
		await expect(
			page.getByRole('dialog').getByText(sharedLocationLabel(shared!, name), { exact: true })
		).toBeVisible();

		await confirmRegister(
			page,
			name,
			/LAN 内の誰でも、この 3\s個すべてを閲覧できます|Anyone on the LAN can view all 3 of these files/
		);
	} finally {
		await removeContentIfPresent(page, name);
		removeFixtureTree(tree);
	}
});

test('アーカイブ: パスを辿って選び、索引される件数を確かめてから登録できる', async ({ page }) => {
	const shared = await sharedContentRoot(page.request);
	test.skip(shared === null, FIXTURE_UNREACHABLE);

	const name = uniqueId('e2e-register-archive');
	const tree = createFixtureTree(name);
	try {
		await openCreateForm(page, 'archive');
		await pickDirectory(page, shared!, name);
		await confirmRegister(page, name, /3\s個のファイルが索引されます|3 files will be indexed/);
		// 作成に続けてスキャンまで済んでいて、再スキャンを押さなくてもアイテムが並ぶ。
		const id = page.url().split('/').pop();
		const items = (await send(page.request, 'get', `/contents/${id}/items`)) as unknown[];
		expect(items).toHaveLength(3);
	} finally {
		await removeContentIfPresent(page, name);
		removeFixtureTree(tree);
	}
});

test('アーカイブ: 作成後のスキャンだけが失敗したら、作成は残し、再スキャンを案内する', async ({
	page
}) => {
	const shared = await sharedContentRoot(page.request);
	test.skip(shared === null, FIXTURE_UNREACHABLE);

	await page.route(/\/api\/v1\/contents\/\d+\/rescan$/, (route) =>
		route.fulfill({ status: 500, json: { error: { code: 'internal_error', message: '' } } })
	);
	const name = uniqueId('e2e-register-archive-scan-failed');
	const tree = createFixtureTree(name);
	try {
		await openCreateForm(page, 'archive');
		await pickDirectory(page, shared!, name);
		await page.getByRole('button', { name: CONTENT_SUBMIT_BUTTON_NAME }).click();
		await page
			.getByRole('dialog', { name: CONFIRM_REGISTER_NAME })
			.getByRole('button', { name: REGISTER_BUTTON_NAME })
			.click();

		const failed = page.getByRole('alertdialog', {
			name: /^ファイルをスキャンできませんでした$|^Couldn't scan the files$/
		});
		await expect(
			failed.getByText(/コンテンツは作成しました|The content was created/)
		).toBeVisible();
		// 作成は残っていて、一覧に行が出ている。
		await expect(page).toHaveURL(/\/admin\/contents$/);
		const contents = (await send(page.request, 'get', '/admin/contents')) as { title: string }[];
		expect(contents.some((content) => content.title === name)).toBe(true);
	} finally {
		await removeContentIfPresent(page, name);
		removeFixtureTree(tree);
	}
});

test('確認で「戻る」を押すと登録せず、フォームに戻る', async ({ page }) => {
	const shared = await sharedContentRoot(page.request);
	test.skip(shared === null, FIXTURE_UNREACHABLE);

	const name = uniqueId('e2e-register-back');
	const tree = createFixtureTree(name);
	try {
		await openCreateForm(page, 'folder');
		await pickDirectory(page, shared!, name);
		await page.getByRole('button', { name: CONTENT_SUBMIT_BUTTON_NAME }).click();

		const confirm = page.getByRole('dialog', { name: CONFIRM_REGISTER_NAME });
		await confirm.getByRole('button', { name: /^戻る$|^Back$/ }).click();
		await expect(confirm).toHaveCount(0);
		// 作成ダイアログの2段目に、入力と選んだパスが残っている。
		const form = page.getByRole('dialog', { name: /^フォルダーを追加$|^Add Folder$/ });
		// タイトルは入力させない (選んだフォルダーの名前で付く)。
		await expect(form.getByLabel(TITLE_LABEL)).toHaveCount(0);
		await expect(form.getByText(sharedLocationLabel(shared!, name), { exact: true })).toBeVisible();
		await expect(form.getByRole('button', { name: CONTENT_SUBMIT_BUTTON_NAME })).toBeEnabled();
		await expect(page).toHaveURL(/\/admin\/contents$/);
		// 登録されていない。
		const contents = (await send(page.request, 'get', '/admin/contents')) as { title: string }[];
		expect(contents.some((content) => content.title === name)).toBe(false);
	} finally {
		await removeContentIfPresent(page, name);
		removeFixtureTree(tree);
	}
});

test('ファイル: 選んだファイルをアップロードして登録できる', async ({ page }) => {
	const title = uniqueId('e2e-register-file');
	const fileName = `${title}.txt`;
	try {
		await openCreateForm(page, 'file');
		await page.getByLabel(requiredLabel('ファイル', 'File')).setInputFiles({
			name: fileName,
			mimeType: 'text/plain',
			buffer: Buffer.from('e2e')
		});
		await submitCreateForm(page, title);
		// 見出しの下と「現在のファイル」の2か所に出る。
		await expect(page.getByText(fileName, { exact: true })).toBeVisible();
	} finally {
		await removeContentIfPresent(page, title);
	}
});

test('公開できるフォルダーが0件なら、フォルダーとアーカイブは選べず、理由が出る', async ({
	page
}) => {
	// 登録済みのフォルダーは他のテストと共有しているので消さない。
	// 0件かどうかを確かめる問い合わせ (上位の一覧) だけを空にして、画面の出し分けを見る。
	await page.route(
		(url) => url.pathname === '/api/v1/admin/fs/dirs' && url.searchParams.get('path') === '',
		(route) =>
			route.fulfill({
				json: { path: '', parent: null, root: null, selectable: false, entries: [] }
			})
	);
	await page.goto('/admin/contents');
	await openAddDialog(page, CONTENT_ADD_BUTTON_NAME);

	const dialog = page.getByRole('dialog');
	await expect(dialog.getByRole('button', { name: CONTENT_TYPE_NAME.folder })).toBeDisabled();
	await expect(dialog.getByRole('button', { name: CONTENT_TYPE_NAME.archive })).toBeDisabled();
	await expect(dialog.getByRole('button', { name: CONTENT_TYPE_NAME.link })).toBeEnabled();
	await expect(
		dialog.getByText(/「公開できるフォルダー」を登録すると|Register a shared folder/)
	).toBeVisible();
});
