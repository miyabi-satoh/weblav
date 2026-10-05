// 権限と公開範囲の境目を、画面の側で確かめる (管理者ログイン済みを前提に、未ログインと編集者の
// コンテキストを別に作る)。決まりそのものは API テスト (tests/api/) が確かめているので、ここでは
// 「見えない・押せない・ログインへ送られる」が画面に現れているかだけを見る。
// 準備と後片付けは API で行う (→ api-helpers.ts)。作ったものは途中で失敗しても必ず片付ける。
import { test, expect, type Page } from '@playwright/test';
import { ADMIN_PASSWORD, ADMIN_USERNAME } from './auth';
import { logInByApi, send, withContentByApi, withUserByApi } from './api-helpers';
import { publishItems, rescanArchive, withDirectoryContent } from './fixture-helpers';
import {
	ADD_AXIS_BUTTON_NAME,
	addAxisValue,
	axisValuesForm,
	itemPublishedSwitch,
	saveAxisValues,
	selectAxis
} from './archive-helpers';
import {
	anonymousContext,
	CONTENT_ADD_BUTTON_NAME,
	CONTENT_TYPE_NAME,
	contentRow,
	DELETE_MENU_ITEM_NAME,
	fillLogInForm,
	ITEMS_TAB_NAME,
	ROW_ACTIONS_BUTTON_NAME,
	SAVE_BUTTON_NAME,
	saveForm,
	TITLE_LABEL,
	uniqueId,
	VISIBILITY_LABEL,
	VISIBILITY_PRIVATE_OPTION_NAME,
	waitForDialog
} from './helpers';

// 待ち続けて制限時間を使い切ると、後片付けまで時間切れになる (→ archive.e2e.ts)。
test.use({ actionTimeout: 10_000, navigationTimeout: 10_000 });

const EDITOR_PASSWORD = 'E2eEditor!2026Test';

/** 管理画面のタブ (admin にだけ出る)。 */
function adminTabs(page: Page) {
	return page.getByRole('navigation', { name: /^管理$|^Admin$/ });
}

test('未ログイン: 要ログインのグループを開くとログインへ送られ、ログインすると戻る', async ({
	page,
	browser
}) => {
	const title = uniqueId('e2e-authenticated-group');
	await withContentByApi(
		page.request,
		{ type: 'group', title, visibility: 'authenticated' },
		async (group) => {
			const anon = await anonymousContext(browser);
			try {
				const anonPage = await anon.newPage();
				await anonPage.goto(`/groups/${group.id}`);
				await anonPage.waitForURL(
					(url) =>
						url.pathname === '/login' && url.searchParams.get('redirect') === `/groups/${group.id}`
				);

				await fillLogInForm(anonPage, ADMIN_USERNAME, ADMIN_PASSWORD);
				await anonPage.waitForURL((url) => url.pathname === `/groups/${group.id}`);
				await expect(anonPage.getByRole('heading', { level: 1, name: title })).toBeVisible();
			} finally {
				await anon.close();
			}
		}
	);
});

test('非表示: 一覧には出ないが、ログインした人は URL で開け、未ログインはログインへ送られる', async ({
	page,
	browser
}) => {
	const title = uniqueId('e2e-hidden-group');
	const publicTitle = uniqueId('e2e-public-group');
	await withContentByApi(
		page.request,
		{ type: 'group', title, visibility: 'hidden' },
		async (group) => {
			await withContentByApi(page.request, { type: 'group', title: publicTitle }, async () => {
				const anon = await anonymousContext(browser);
				try {
					// 「出ない」の確かめが掴み方の誤りで通らないよう、同じ掴み方で「誰でも」は出ることも見る。
					await page.goto('/');
					await expect(page.getByRole('link', { name: publicTitle })).toBeVisible();
					await expect(page.getByRole('link', { name: title })).toHaveCount(0);

					await page.goto(`/groups/${group.id}`);
					await expect(page.getByRole('heading', { level: 1, name: title })).toBeVisible();

					const anonPage = await anon.newPage();
					await anonPage.goto(`/groups/${group.id}`);
					await anonPage.waitForURL((url) => url.pathname === '/login');
				} finally {
					await anon.close();
				}
			});
		}
	);
});

test('親グループ: 要ログインのグループの中の「誰でも」は、一覧に設定を出し、編集ページで実際の見え方を示す', async ({
	page
}) => {
	const groupTitle = uniqueId('e2e-parent-group');
	const linkTitle = uniqueId('e2e-child-link');
	await withContentByApi(
		page.request,
		{
			type: 'group',
			title: groupTitle,
			visibility: 'authenticated'
		},
		async (group) => {
			await withContentByApi(
				page.request,
				{
					type: 'link',
					title: linkTitle,
					visibility: 'public',
					parentId: group.id
				},
				async (link) => {
					// 一覧は各行の設定 (誰でも) だけを出す。
					await page.goto('/admin/contents');
					const row = contentRow(page, linkTitle);
					await expect(row).toContainText(/誰でも|Anyone/);
					await expect(row).not.toContainText(/要ログイン|Sign-in required/);

					// 編集ページでは、親グループによる実際の見え方 (要ログイン) を公開範囲の下に書く。
					await page.goto(`/admin/contents/${link.id}`);
					await expect(
						page.getByText(
							/親グループの公開範囲により、実際は「要ログイン」になります。|Because of the parent group's visibility, this is actually "Sign-in required"\./
						)
					).toBeVisible();
				}
			);
		}
	);
});

test('公開範囲の選択肢: グループには「本人のみ」が出ず、リンクには出る', async ({ page }) => {
	const groupTitle = uniqueId('e2e-group-options');
	const linkTitle = uniqueId('e2e-link-options');
	await withContentByApi(page.request, { type: 'group', title: groupTitle }, async (group) => {
		await withContentByApi(page.request, { type: 'link', title: linkTitle }, async (link) => {
			for (const [id, privateCount] of [
				[group.id, 0],
				[link.id, 1]
			] as const) {
				await page.goto(`/admin/contents/${id}`);
				await page.getByLabel(VISIBILITY_LABEL).click();
				await expect(page.getByRole('option', { name: /^非表示$|^Hidden$/ })).toBeVisible();
				await expect(
					page.getByRole('option', { name: VISIBILITY_PRIVATE_OPTION_NAME })
				).toHaveCount(privateCount);
				await page.keyboard.press('Escape');
			}
		});
	});
});

test('編集者: 管理のタブが出ず、ユーザー管理とサイト設定はコンテンツ管理へ戻されるが、追加の種類は全部出る', async ({
	page,
	browser
}) => {
	const username = uniqueId('e2e-editor');
	// 管理者にはタブが出る。編集者で「出ない」ことを見る掴み方の対照。
	await page.goto('/admin/contents');
	await expect(adminTabs(page)).toBeVisible();
	await withUserByApi(page.request, username, EDITOR_PASSWORD, async () => {
		const editor = await anonymousContext(browser);
		try {
			await logInByApi(editor.request, username, EDITOR_PASSWORD);
			const editorPage = await editor.newPage();

			await editorPage.goto('/admin/contents');
			await expect(editorPage.getByRole('button', { name: CONTENT_ADD_BUTTON_NAME })).toBeVisible();
			await expect(adminTabs(editorPage)).toHaveCount(0);

			for (const path of ['/admin/users', '/admin/settings']) {
				await editorPage.goto(path);
				await editorPage.waitForURL((url) => url.pathname === '/admin/contents');
			}

			// フォルダとアーカイブも編集者に開いている (→ docs/access.md「ロールと操作」)。
			await editorPage.getByRole('button', { name: CONTENT_ADD_BUTTON_NAME }).click();
			await waitForDialog(editorPage);
			const dialog = editorPage.getByRole('dialog');
			for (const name of [
				CONTENT_TYPE_NAME.link,
				CONTENT_TYPE_NAME.folder,
				CONTENT_TYPE_NAME.archive
			]) {
				await expect(dialog.getByRole('button', { name })).toBeEnabled();
			}
		} finally {
			await editor.close();
		}
	});
});

test('編集者: 他人のアーカイブは見るだけで、管理者が作成者を付け替えると軸まで設定できる', async ({
	page,
	browser
}) => {
	const username = uniqueId('e2e-editor');
	await withDirectoryContent(
		page.request,
		{ type: 'archive', namePrefix: 'e2e-editor-archive' },
		async (archive, title) => {
			await send(page.request, 'post', `/contents/${archive.id}/axes`, {
				name: '種類',
				source: 'filenameWord',
				position: 0
			});
			await rescanArchive(page.request, archive.id);

			// 管理者には「軸を追加...」が出る。編集者で「出ない」ことを見る掴み方の対照。
			await page.goto(`/admin/contents/${archive.id}/axes`);
			await expect(page.getByRole('button', { name: ADD_AXIS_BUTTON_NAME })).toBeVisible();

			await withUserByApi(page.request, username, EDITOR_PASSWORD, async () => {
				const editor = await anonymousContext(browser);
				try {
					await logInByApi(editor.request, username, EDITOR_PASSWORD);
					const editorPage = await editor.newPage();

					// 一覧の⋮メニューは「開く」だけになり、削除は出ない。
					await editorPage.goto('/admin/contents');
					await contentRow(editorPage, title)
						.getByRole('button', { name: ROW_ACTIONS_BUTTON_NAME })
						.click();
					await expect(editorPage.getByRole('menuitem', { name: /^開く$|^Open$/ })).toBeVisible();
					await expect(
						editorPage.getByRole('menuitem', { name: DELETE_MENU_ITEM_NAME })
					).toHaveCount(0);
					await editorPage.keyboard.press('Escape');

					// 基本情報は見るだけ。理由を見出しの下に出し、保存ボタンを出さない (→ docs/ui.md「UI 全般」)。
					await editorPage.goto(`/admin/contents/${archive.id}`);
					await expect(editorPage.getByTestId('view-only-note')).toBeVisible();
					await expect(editorPage.getByLabel(TITLE_LABEL)).toBeDisabled();
					// 選択 (bits-ui) は fieldset では止まらないので、開かないことも見る。
					await editorPage
						.getByRole('button', { name: /^親グループ$|^Parent group$/ })
						.click({ force: true });
					await expect(editorPage.getByRole('listbox')).toHaveCount(0);
					await expect(editorPage.getByRole('button', { name: SAVE_BUTTON_NAME })).toHaveCount(0);

					await editorPage.getByRole('link', { name: ITEMS_TAB_NAME }).click();
					await expect(itemPublishedSwitch(editorPage, '2024/answer.pdf')).toBeDisabled();
					await expect(
						editorPage.getByRole('button', { name: /再スキャンする|Rescan/ })
					).toHaveCount(0);

					// 値の辞書は見えるが、保存ボタンは出ない (selectAxis は保存ボタンで読み込みを待つので使わない)。
					await editorPage.getByRole('link', { name: /^軸$|^Axes$/ }).click();
					await editorPage
						.getByRole('button')
						.filter({ has: editorPage.getByText('種類', { exact: true }) })
						.click();
					await expect(axisValuesForm(editorPage)).toBeVisible();
					await expect(editorPage.getByRole('button', { name: ADD_AXIS_BUTTON_NAME })).toHaveCount(
						0
					);
					await expect(editorPage.getByRole('button', { name: SAVE_BUTTON_NAME })).toHaveCount(0);

					// 管理者が作成者を編集者に付け替える。
					await page.goto(`/admin/contents/${archive.id}`);
					await page.getByRole('button', { name: /^作成者$|^Creator$/ }).click();
					await page.getByRole('option', { name: username }).click();
					await saveForm(page);

					await editorPage.reload();
					await expect(editorPage.getByTestId('view-only-note')).toHaveCount(0);
					await expect(
						editorPage.getByRole('button', { name: ADD_AXIS_BUTTON_NAME })
					).toBeVisible();
					await selectAxis(editorPage, '種類');
					await addAxisValue(editorPage, 'listening', 'リスニング');
					await saveAxisValues(editorPage, '種類');

					await editorPage.getByRole('link', { name: ITEMS_TAB_NAME }).click();
					const toggle = itemPublishedSwitch(editorPage, '2024/answer.pdf');
					await toggle.click();
					await expect(toggle).toHaveAttribute('aria-checked', 'true');
				} finally {
					await editor.close();
				}
			});
		}
	);
});

test('アーカイブの閲覧: 非公開のアイテムは並ばない', async ({ page }) => {
	await withDirectoryContent(
		page.request,
		{ type: 'archive', namePrefix: 'e2e-unpublished' },
		async (archive) => {
			const items = await rescanArchive(page.request, archive.id);
			await publishItems(page.request, archive.id, items, ['2024/answer.pdf']);

			await page.goto(`/archives/${archive.id}`);
			await expect(page.getByText(/^1\s件$|^1 items$/)).toBeVisible();
			await expect(page.getByRole('link', { name: /answer\.pdf/ })).toBeVisible();
			await expect(page.getByRole('button', { name: /listening\.mp3/ })).toHaveCount(0);
		}
	);
});
