// 権限と公開範囲の境目を、画面の側で確かめる (管理者ログイン済みを前提に、未ログインと編集者の
// コンテキストを別に作る)。決まりそのものは API テスト (tests/api/) が確かめているので、ここでは
// 「見えない・押せない・ログインへ送られる」が画面に現れているかだけを見る。
// 準備と後片付けは API で行う (→ api-helpers.ts)。作ったものは途中で失敗しても必ず片付ける。
import { test, expect, type Page } from '@playwright/test';
import { ADMIN_PASSWORD, ADMIN_USERNAME } from './auth';
import { logInByApi, send, withContentByApi, withUserByApi } from './api-helpers';
import { rescanArchive, withDirectoryContent } from './fixture-helpers';
import { ADD_AXIS_BUTTON_NAME, addAxisValue, saveAxisValues, selectAxis } from './archive-helpers';
import {
	anonymousContext,
	basicSaveButton,
	contentRow,
	fillLogInForm,
	ITEMS_TAB_NAME,
	ACCOUNT_MENU_ITEM_NAME,
	saveForm,
	uniqueId,
	VISIBILITY_LABEL,
	VISIBILITY_PRIVATE_OPTION_NAME,
	browseLink
} from './helpers';

// 待ち続けて制限時間を使い切ると、後片付けまで時間切れになる (→ archive.e2e.ts)。
test.use({ actionTimeout: 10_000, navigationTimeout: 10_000 });

const EDITOR_PASSWORD = 'E2eEditor!2026Test';

/** ユーザーメニューの管理画面の入口 (admin にだけ出る)。 */
const ADMIN_MENU_ITEM_NAME = /^管理ページ$|^Admin$/;

/** 管理画面のタブ (admin にだけ出る)。 */
function adminTabs(page: Page) {
	return page.getByRole('navigation', { name: /^管理ページ$|^Admin$/ });
}

test('未ログイン: 「ログインした人」のグループを開くとログインへ送られ、ログインすると戻る', async ({
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

test('非表示: 閲覧の一覧には出ず、非表示の一覧から開け、未ログインはログインへ送られる', async ({
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
					await expect(browseLink(page, publicTitle)).toBeVisible();
					await expect(browseLink(page, title)).toHaveCount(0);

					// 非表示の一覧は閲覧の一覧と同じ部品なので、行を押すとその閲覧画面が開く。
					await page.goto('/hidden');
					await browseLink(page, title).click();
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

test('親グループ: 「ログインした人」のグループの中の「誰でも」は、一覧に設定を出し、編集ページで実際の見え方を示す', async ({
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
					await expect(row).toContainText(/誰でも|Everyone/);
					await expect(row).not.toContainText(/ログインした人|Logged-in users/);

					// 編集ページでは、親グループによる実際の見え方 (「ログインした人」) を公開範囲の下に書く。
					await page.goto(`/contents/${link.id}`);
					await expect(
						page.getByText(
							/親のグループの公開範囲により、実際には「ログインした人」になります。|Because of the parent group's visibility, this is actually “Logged-in users”\./
						)
					).toBeVisible();
				}
			);
		}
	);
});

test('公開範囲の選択肢: 「作成者のみ」のグループの中のものには「作成者のみ」だけが出る', async ({
	page
}) => {
	const groupTitle = uniqueId('e2e-private-group');
	const linkTitle = uniqueId('e2e-link-in-private');
	await withContentByApi(
		page.request,
		{ type: 'group', title: groupTitle, visibility: 'private' },
		async (group) => {
			await withContentByApi(
				page.request,
				{ type: 'link', title: linkTitle, url: 'https://example.com', parentId: group.id },
				async (link) => {
					await page.goto(`/contents/${link.id}`);
					await page.getByLabel(VISIBILITY_LABEL).click();
					await expect(page.getByRole('option')).toHaveCount(1);
					await expect(
						page.getByRole('option', { name: VISIBILITY_PRIVATE_OPTION_NAME })
					).toBeVisible();
					await page.keyboard.press('Escape');
				}
			);
		}
	);
});

test('管理者: 他人の「作成者のみ」でも、公開範囲をすべて選べて作成者も選び直せる', async ({
	page,
	browser
}) => {
	const username = uniqueId('e2e-private-owner');
	await withUserByApi(page.request, username, EDITOR_PASSWORD, async () => {
		const owner = await anonymousContext(browser);
		try {
			await logInByApi(owner.request, username, EDITOR_PASSWORD);
			await withContentByApi(
				owner.request,
				{
					type: 'link',
					title: uniqueId('e2e-others-private'),
					url: 'https://example.com',
					visibility: 'private'
				},
				async (link) => {
					await page.goto(`/contents/${link.id}`);
					await page.getByLabel(VISIBILITY_LABEL).click();
					await expect(page.getByRole('option')).toHaveCount(4);
					await page.keyboard.press('Escape');
					await expect(page.getByRole('button', { name: /^作成者$|^Creator$/ })).toBeEnabled();
				}
			);
		} finally {
			await owner.close();
		}
	});
});

test('編集者: 管理画面はどれもホームへ戻され、メニューに入口が出ず、ホームの「追加」から種類を全部選べる', async ({
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

			// `/admin` の下は admin 専用 (→ docs/access.md「ロールと操作」)。
			for (const path of ['/admin/contents', '/admin/users', '/admin/roots', '/admin/settings']) {
				await editorPage.goto(path);
				await editorPage.waitForURL((url) => url.pathname === '/');
			}
			await expect(adminTabs(editorPage)).toHaveCount(0);

			await editorPage.getByRole('button', { name: username, exact: true }).click();
			await expect(
				editorPage.getByRole('menuitem', { name: ACCOUNT_MENU_ITEM_NAME })
			).toBeVisible();
			await expect(editorPage.getByRole('menuitem', { name: ADMIN_MENU_ITEM_NAME })).toHaveCount(0);
			await editorPage.keyboard.press('Escape');

			// フォルダー (ショートカット) も編集者に開いている (→ docs/access.md「ロールと操作」)。
			await editorPage.getByRole('button', { name: /^追加$|^Add$/ }).click();
			for (const name of [
				/^リンク\.\.\.$|^Link\.\.\.$/,
				/^ショートカット\.\.\.$|^Shortcut\.\.\.$/,
				/^グループ\.\.\.$|^Group\.\.\.$/
			]) {
				await expect(editorPage.getByRole('menuitem', { name })).toBeVisible();
			}
		} finally {
			await editor.close();
		}
	});
});

test('編集者: 他人のアーカイブの編集ページは閲覧画面へ送られ、管理者が作成者を付け替えると軸まで設定できる', async ({
	page,
	browser
}) => {
	const username = uniqueId('e2e-editor');
	await withDirectoryContent(
		page.request,
		{ type: 'archive', namePrefix: 'e2e-editor-archive' },
		async (archive) => {
			await send(page.request, 'post', `/contents/${archive.id}/axes`, {
				name: '種類',
				source: 'filenameWord',
				position: 0
			});
			await rescanArchive(page.request, archive.id);

			await withUserByApi(page.request, username, EDITOR_PASSWORD, async () => {
				const editor = await anonymousContext(browser);
				try {
					await logInByApi(editor.request, username, EDITOR_PASSWORD);
					const editorPage = await editor.newPage();

					// 書き換えられない編集ページは、そのライブラリの閲覧画面へ送る (→ docs/ui.md「UI 全般」)。
					await editorPage.goto(`/contents/${archive.id}/axes`);
					await editorPage.waitForURL(new RegExp(`/archives/${archive.id}$`));

					// 管理者が作成者を編集者に付け替える。
					await page.goto(`/contents/${archive.id}`);
					await page.getByRole('button', { name: /^作成者$|^Creator$/ }).click();
					await page.getByRole('option', { name: username }).click();
					await saveForm(page, { button: basicSaveButton(page) });

					await editorPage.goto(`/contents/${archive.id}/axes`);
					await expect(
						editorPage.getByRole('button', { name: ADD_AXIS_BUTTON_NAME })
					).toBeVisible();
					await selectAxis(editorPage, '種類');
					await addAxisValue(editorPage, 'listening', 'リスニング');
					await saveAxisValues(editorPage, '種類');

					await browseLink(editorPage, ITEMS_TAB_NAME).click();
					await expect(
						editorPage.getByRole('button', { name: /再スキャンする|Rescan/ })
					).toBeVisible();
				} finally {
					await editor.close();
				}
			});
		}
	);
});
