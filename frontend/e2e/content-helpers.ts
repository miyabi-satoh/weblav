import { expect, type Page } from '@playwright/test';
import {
	basicSaveButton,
	chooseContentType,
	clickRowMenuItem,
	confirmDelete,
	CONTENT_ADD_BUTTON_NAME,
	ADD_SUBMIT_BUTTON_NAME,
	CONTENT_TYPE_NAME,
	contentRow,
	DELETE_MENU_ITEM_NAME,
	openAddDialog,
	requiredLabel,
	saveForm,
	TITLE_LABEL,
	VISIBILITY_LABEL,
	waitForDialog,
	BROWSE_ADD_BUTTON_NAME,
	BROWSE_ADD_MENU_ITEM_NAME
} from './helpers';

/**
 * 作成ダイアログを開き、種別を選んで2段目へ進む。
 * `parentTitle` を渡すと、そのグループの行の「この中に追加...」から開く。
 */
export async function openCreateForm(
	page: Page,
	type: keyof typeof CONTENT_TYPE_NAME,
	parentTitle?: string
) {
	await page.goto('/admin/contents');
	if (parentTitle) {
		await clickRowMenuItem(page, contentRow(page, parentTitle), /中に追加|Add inside/);
		await waitForDialog(page);
	} else {
		await openAddDialog(page, CONTENT_ADD_BUTTON_NAME);
	}
	await chooseContentType(page, type);
}

/**
 * ホームの見出しの「追加」から、リンクの作成の窓を開く。管理画面は admin 専用なので、一般ユーザーはここから作る。
 */
async function openLinkFormFromHome(page: Page) {
	await page.goto('/');
	await page.getByRole('button', { name: BROWSE_ADD_BUTTON_NAME }).click();
	await page.getByRole('menuitem', { name: BROWSE_ADD_MENU_ITEM_NAME.link }).click();
	await waitForDialog(page);
}

/**
 * 作成を確定し、移った先の編集ページで作成できたことを確かめる。
 * タイトルは group 以外が自動で付くので、分かっているときだけ `title` を渡す。
 */
export async function submitCreateForm(page: Page, title?: string) {
	// 閲覧の画面の見出しにも「追加」があり、英語では同じ名前になるので、窓の中のボタンに限る。
	await page.getByRole('dialog').getByRole('button', { name: ADD_SUBMIT_BUTTON_NAME }).click();
	await expectContentEditPage(page, title);
}

/** 作成できて、そのコンテンツの編集ページへ移ったこと。 */
export async function expectContentEditPage(page: Page, title?: string) {
	await page.waitForURL(/\/contents\/\d+$/);
	if (title === undefined) {
		await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
	} else {
		await expect(page.getByRole('heading', { level: 1, name: title })).toBeVisible();
	}
}

export async function createLinkContent(
	page: Page,
	title: string,
	options: { url?: string; parentTitle?: string; visibility?: RegExp; fromHome?: boolean } = {}
) {
	if (options.fromHome) {
		await openLinkFormFromHome(page);
	} else {
		await openCreateForm(page, 'link', options.parentTitle);
	}
	// 既定は、サーバーが取りに行かない (公開アドレスではない) URL。タイトルの取得で待たされず、
	// 結果も環境の通信に左右されない。
	await page
		.getByLabel(requiredLabel('URL'))
		.fill(options.url ?? 'http://localhost/e2e-smoke-test');
	await submitCreateForm(page);
	// リンクの作成ダイアログにタイトルと公開範囲は無いので、移った編集ページで付ける。
	// 公開範囲の省略時は既定の「誰でも」のまま。
	await page.getByLabel(TITLE_LABEL).fill(title);
	if (options.visibility) {
		await page.getByLabel(VISIBILITY_LABEL).click();
		await page.getByRole('option', { name: options.visibility }).click();
	}
	await saveForm(page, { button: basicSaveButton(page) });
	await expectContentEditPage(page, title);
}

export async function createGroupContent(page: Page, title: string) {
	await openCreateForm(page, 'group');
	await page.getByLabel(TITLE_LABEL).fill(title);
	await submitCreateForm(page, title);
}

export async function deleteContentByTitle(page: Page, title: string) {
	await page.goto('/admin/contents');
	const row = contentRow(page, title);
	await clickRowMenuItem(page, row, DELETE_MENU_ITEM_NAME);
	await confirmDelete(page);
	await expect(row).toHaveCount(0);
}

/**
 * 後片付け専用。作成の途中で失敗して行が無い場合もあるので、残っているときだけ消す。
 * テスト本体の失敗を覆い隠さないよう、ここでの失敗は警告に留める。
 */
export async function removeContentIfPresent(page: Page, title: string) {
	try {
		await page.goto('/admin/contents');
		// 一覧はloadの完了後に描画されるため、見出しを待ってから行の有無を数える。
		await page.getByRole('heading', { level: 1 }).first().waitFor();
		if ((await contentRow(page, title).count()) > 0) {
			await deleteContentByTitle(page, title);
		}
	} catch (err) {
		console.warn(`コンテンツの後片付けに失敗しました (${title}):`, err);
	}
}
