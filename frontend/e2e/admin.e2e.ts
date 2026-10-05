// 管理者ログイン済み(auth.setup.tsが保存したstorageState)を前提とするスモークテスト。
// どのインスタンスにも既存の管理者アカウントがあること以外は前提にしないため、
// フォルダ/アーカイブ等ファイルシステムの実体を要する画面はここでは扱わない
// (フィクスチャを作って登録する archive.e2e.ts に分けている)。
//
// 書き込み系のテストは、作成したコンテンツ/ユーザーを必ず後片付けする(プロジェクトの
// 運用DBを汚さないため)。作成の途中で失敗しても片付けが走るよう、作成の前から
// try/finally で囲む。
import { test, expect } from '@playwright/test';
import { withContentByApi } from './api-helpers';
import { ADMIN_USERNAME } from './auth';
import {
	anonymousContext,
	AUDIO_PLAYER_REGION_NAME,
	BROWSE_LAYOUT_LIST_NAME,
	BROWSE_LAYOUT_TILE_NAME,
	CANCEL_BUTTON_NAME,
	PASSWORD_CONFIRM_LABEL,
	PASSWORD_LABEL,
	RESET_PASSWORD_MENU_ITEM_NAME,
	TITLE_LABEL,
	clickRowMenuItem,
	confirmDelete,
	CONTENT_ADD_BUTTON_NAME,
	contentRow,
	CONTENTS_NAV_LINK_NAME,
	DELETE_MENU_ITEM_NAME,
	dragContentRowOnto,
	dragContentRowToRoot,
	startDraggingContentRow,
	logIn,
	LOGIN_USERNAME_LABEL,
	SAVE_BUTTON_NAME,
	saveForm,
	uniqueId,
	userRow,
	VISIBILITY_LABEL,
	VISIBILITY_PRIVATE_OPTION_NAME,
	waitForDialog,
	PASSWORD_HASH_TIMEOUT
} from './helpers';
import {
	createGroupContent,
	createLinkContent,
	deleteContentByTitle,
	removeContentIfPresent
} from './content-helpers';
import { createUser, deleteUserByUsername, removeUserIfPresent } from './user-helpers';

test('管理画面: コンテンツ一覧が表示される', async ({ page }) => {
	await page.goto('/admin/contents');
	await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible();
	await expect(page.getByRole('button', { name: CONTENT_ADD_BUTTON_NAME })).toBeVisible();
});

test('ログイン済みでログイン画面を開くと行き先へ送られる', async ({ page }) => {
	// フォームをそのまま出すと「ログアウトされたのか」と考えさせてしまう。
	// → frontend/src/routes/login/+page.ts
	// SPA なのでリダイレクトはクライアント側で起きる。goto の解決を待つだけでは早い。
	await page.goto('/login');
	await page.waitForURL((url) => url.pathname === '/');
	await expect(page.getByLabel(LOGIN_USERNAME_LABEL)).toHaveCount(0);

	// `?redirect=` があればそこへ送る。ログイン成功後の遷移と同じ規則。
	await page.goto('/login?redirect=%2Fadmin%2Fcontents');
	await page.waitForURL((url) => url.pathname === '/admin/contents');
	await expect(page.getByRole('button', { name: CONTENT_ADD_BUTTON_NAME })).toBeVisible();

	// このページ自身を戻り先に指定されても、そこへは送らずトップへ落とす。
	await page.goto('/login?redirect=%2Flogin');
	await page.waitForURL((url) => url.pathname === '/');
});

test('管理画面: ユーザー一覧に自分自身が表示される', async ({ page }) => {
	await page.goto('/admin/users');
	await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible();
	await expect(page.getByRole('cell', { name: ADMIN_USERNAME, exact: true })).toBeVisible();
});

test('画面を移る途中は、移る先の読み込みが終わるまで上端にバーが出る', async ({ page }) => {
	let release!: () => void;
	const released = new Promise<void>((resolve) => (release = resolve));
	await page.route(
		(url) => url.pathname === '/api/v1/admin/users',
		async (route) => {
			await released;
			await route.continue();
		}
	);
	await page.goto('/admin/contents');
	const progress = page.getByRole('progressbar', { name: /^読み込み中$|^Loading$/ });
	await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible();
	await expect(progress).toHaveCount(0);

	await page.getByRole('link', { name: /^ユーザー$|^Users$/ }).click();
	await expect(progress).toBeVisible();

	release();
	await page.waitForURL(/\/admin\/users$/);
	await expect(progress).toHaveCount(0);
});

test('管理画面: コンテンツを登録して削除できる', async ({ page }) => {
	const title = uniqueId('E2Eスモークテスト');
	try {
		await createLinkContent(page, title);
		await deleteContentByTitle(page, title);
	} finally {
		await removeContentIfPresent(page, title);
	}
});

test('管理画面: コンテンツを編集できる', async ({ page }) => {
	const title = uniqueId('E2E編集前');
	const editedTitle = uniqueId('E2E編集後');

	try {
		await createLinkContent(page, title);
		await page.goto('/admin/contents');
		await clickRowMenuItem(page, contentRow(page, title), /^編集$|^Edit$/);
		await page.waitForURL(/\/admin\/contents\/\d+$/);
		await page.getByLabel(TITLE_LABEL).fill(editedTitle);

		// 保存していない変更があるうちは、一覧へ戻ろうとすると確認が出て留まれる。
		await page.getByRole('link', { name: CONTENTS_NAV_LINK_NAME }).first().click();
		await page.getByRole('alertdialog').getByRole('button', { name: CANCEL_BUTTON_NAME }).click();
		await expect(page).toHaveURL(/\/admin\/contents\/\d+$/);

		await saveForm(page);
		await expect(page.getByRole('heading', { level: 1, name: editedTitle })).toBeVisible();

		// 保存した後は確認を出さずに移れる。
		await page.getByRole('link', { name: CONTENTS_NAV_LINK_NAME }).first().click();
		await page.waitForURL(/\/admin\/contents$/);
		await expect(contentRow(page, editedTitle)).toBeVisible();
		await expect(contentRow(page, title)).toHaveCount(0);
	} finally {
		await removeContentIfPresent(page, editedTitle);
		await removeContentIfPresent(page, title);
	}
});

// トップ・グループの一覧(content-list.svelte)では、新規タブで開く種別(link/file)の
// リンク名に sr-only の「新しいタブで開く」が付く。uniqueIdでタイトルが一意なので
// exact を付けずに部分一致で探す。

test('グループ配下のコンテンツが一覧できる', async ({ page }) => {
	const groupTitle = uniqueId('E2Eグループ');
	const childTitle = uniqueId('E2E子コンテンツ');

	try {
		await createGroupContent(page, groupTitle);
		await createLinkContent(page, childTitle, { parentTitle: groupTitle });
		// グループのidを取得せず、ホームの一覧から辿ることで結合を減らす。
		await page.goto('/');
		await page.getByRole('link', { name: groupTitle }).click();
		await expect(page.getByRole('link', { name: childTitle })).toBeVisible();
	} finally {
		await removeContentIfPresent(page, childTitle);
		await removeContentIfPresent(page, groupTitle);
	}
});

test('一覧: タイルに切り替えると、読み込み直してもほかの一覧でもタイルのまま', async ({ page }) => {
	const groupTitle = uniqueId('E2Eグループ');
	const childTitle = uniqueId('E2E子コンテンツ');

	try {
		await createGroupContent(page, groupTitle);
		await createLinkContent(page, childTitle, { parentTitle: groupTitle });
		await page.goto('/');
		await expect(page.getByRole('radio', { name: BROWSE_LAYOUT_LIST_NAME })).toBeChecked();
		await page.getByRole('radio', { name: BROWSE_LAYOUT_TILE_NAME }).click();

		await page.reload();
		await expect(page.getByRole('radio', { name: BROWSE_LAYOUT_TILE_NAME })).toBeChecked();
		await page.getByRole('link', { name: groupTitle }).click();
		await expect(page.getByRole('link', { name: childTitle })).toBeVisible();
		await expect(page.getByRole('radio', { name: BROWSE_LAYOUT_TILE_NAME })).toBeChecked();

		// 選んでいる側をもう一度押しても、選択は外れない。
		await page.getByRole('radio', { name: BROWSE_LAYOUT_TILE_NAME }).click();
		await expect(page.getByRole('radio', { name: BROWSE_LAYOUT_TILE_NAME })).toBeChecked();
	} finally {
		await removeContentIfPresent(page, childTitle);
		await removeContentIfPresent(page, groupTitle);
	}
});

test('ホーム: 公開コンテンツが管理者にも未ログインにも表示される', async ({ page, browser }) => {
	// visibilityは既定で"public"(→ content-helpers.tsのcreateLinkContentが変更しない)。
	const title = uniqueId('E2E公開コンテンツ');

	try {
		await createLinkContent(page, title);
		await page.goto('/');
		await expect(page.getByRole('link', { name: title })).toBeVisible();

		const anonContext = await anonymousContext(browser);
		try {
			const anonPage = await anonContext.newPage();
			await anonPage.goto('/');
			await expect(anonPage.getByRole('link', { name: title })).toBeVisible();
		} finally {
			await anonContext.close();
		}
	} finally {
		await removeContentIfPresent(page, title);
	}
});

test('ホーム: 音声のファイルは、新規タブではなくページ内のプレイヤーで開く', async ({ page }) => {
	const title = uniqueId('E2E音声ファイル');
	await withContentByApi(
		page.request,
		{
			title,
			fileName: 'e2e.mp3',
			mimeType: 'audio/mpeg',
			buffer: Buffer.from('e2e')
		},
		async ({ id }) => {
			// 中身は再生できなくてよい (振り分けは拡張子で決まり、画面の動きだけを見る)。
			await page.goto('/');
			await expect(page.getByRole('link', { name: title })).toHaveCount(0);
			await page.getByRole('button', { name: title }).click();
			await expect(page.getByRole('region', { name: AUDIO_PLAYER_REGION_NAME })).toBeVisible();
			await expect(page.locator('audio')).toHaveAttribute(
				'src',
				new RegExp(`/api/v1/contents/${id}/download$`)
			);
		}
	);
});

test('本人のみ: 作成者の一覧にだけ印付きで並び、作成者を削除すると非表示で残る', async ({
	page,
	browser
}) => {
	const username = uniqueId('e2e-private');
	const password = 'E2eSmoke!2026Test';
	const title = uniqueId('E2E本人のみ');
	const ownerContext = await anonymousContext(browser);

	try {
		await createUser(page, username, password);
		const ownerPage = await ownerContext.newPage();
		await logIn(ownerPage, username, password);
		await ownerPage.waitForURL('/');
		await createLinkContent(ownerPage, title, { visibility: VISIBILITY_PRIVATE_OPTION_NAME });

		await ownerPage.goto('/');
		await expect(ownerPage.getByRole('link', { name: title })).toContainText(/本人のみ|Private/);

		// 管理者でも、他人の本人のみは閲覧の一覧に並ばない。見出しの描画を待ってから数える
		// (一覧は load の完了後に見出しと一緒に描画される。DB が空だと一覧そのものが出ないので、一覧は待たない)。
		await page.goto('/');
		await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible();
		await expect(page.getByRole('link', { name: title })).toHaveCount(0);

		await page.goto('/admin/users');
		await clickRowMenuItem(page, userRow(page, username), DELETE_MENU_ITEM_NAME);
		const dialog = page.getByRole('alertdialog');
		await expect(dialog).toContainText(
			/「本人のみ」のコンテンツ 1\s件は、「非表示」に変えて残します|Private contents created by this user \(1\) will be kept as "Hidden"/
		);
		await confirmDelete(page);
		await expect(userRow(page, username)).toHaveCount(0);

		await page.goto('/admin/contents');
		await expect(contentRow(page, title)).toContainText(/非表示|Hidden/);
	} finally {
		await ownerContext.close();
		await removeUserIfPresent(page, username);
		await removeContentIfPresent(page, title);
	}
});

test('管理画面: ユーザーを登録して削除できる', async ({ page }) => {
	const username = uniqueId('e2e-smoke');
	try {
		await createUser(page, username, 'E2eSmoke!2026Test');
		await deleteUserByUsername(page, username);
	} finally {
		await removeUserIfPresent(page, username);
	}
});

test('管理画面: ユーザーのパスワードを再設定できる', async ({ page }) => {
	const username = uniqueId('e2e-reset');
	const newPassword = 'E2eSmoke!2026Reset';

	try {
		await createUser(page, username, 'E2eSmoke!2026Test');
		const row = userRow(page, username);
		await clickRowMenuItem(page, row, RESET_PASSWORD_MENU_ITEM_NAME);
		// ダイアログの描画を待たずに入力すると、入力値が画面の状態に反映されないことがある。
		await waitForDialog(page);
		await page.getByLabel(PASSWORD_LABEL).fill(newPassword);
		await page.getByLabel(PASSWORD_CONFIRM_LABEL).fill(newPassword);
		await page.getByRole('button', { name: SAVE_BUTTON_NAME }).click();
		// 成功するとダイアログが閉じる(→ handleResetSubmit)。
		await expect(page.getByRole('dialog')).toHaveCount(0, { timeout: PASSWORD_HASH_TIMEOUT });
	} finally {
		await removeUserIfPresent(page, username);
	}
});

test('管理画面: サイト設定を保存するとホームの見出しに出る', async ({ page }) => {
	const SITE_NAME_LABEL = /^サイト名$|^Site name$/;
	const HOME_HEADING_LABEL = /^ホームの見出し$|^Home heading$/;
	const siteName = uniqueId('site');
	const homeHeading = uniqueId('heading');

	async function save(values: { siteName: string; homeHeading: string }) {
		await page.goto('/admin/settings');
		await page.getByRole('textbox', { name: SITE_NAME_LABEL }).fill(values.siteName);
		await page.getByRole('textbox', { name: HOME_HEADING_LABEL }).fill(values.homeHeading);
		// 保存は区画ごとにあるので、「ホームの表示」のフォームの中のボタンを押す。
		const form = page.getByRole('form', { name: /^ホームの表示$|^Home page$/ });
		await saveForm(page, { button: form.getByRole('button', { name: SAVE_BUTTON_NAME }) });
	}

	// 運用DBの設定を壊さないよう、元の値に戻す。
	await page.goto('/admin/settings');
	const original = {
		siteName: await page.getByRole('textbox', { name: SITE_NAME_LABEL }).inputValue(),
		homeHeading: await page.getByRole('textbox', { name: HOME_HEADING_LABEL }).inputValue()
	};
	try {
		await save({ siteName, homeHeading });

		await page.goto('/');
		await expect(page.getByRole('heading', { level: 1, name: homeHeading })).toBeVisible();
		await expect(page.getByText(siteName, { exact: true })).toBeVisible();
	} finally {
		await save(original);
	}
});

test('管理画面: コンテンツ一覧の行を押すと編集ページへ移る', async ({ page }) => {
	const title = uniqueId('E2E行を押す');
	try {
		await createLinkContent(page, title);
		await page.goto('/admin/contents');
		// タイトル以外のセル (公開範囲) を押しても移ることを確かめる (→ docs/ui.md「UI 全般」)。
		// セルの上にはタイトルのリンクの押せる範囲が重なるので、要素ではなく座標で押す。
		// タイトルのセルもスマートフォン幅用の公開範囲 (この幅では隠れている) を文字として持つので、
		// 隠れた文字を含まないアクセシブルな名前で探す。
		const cell = contentRow(page, title).getByRole('cell', { name: /^(誰でも|Anyone)$/ });
		const box = await cell.boundingBox();
		if (box === null) throw new Error('公開範囲のセルが見つからない');
		await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
		await page.waitForURL(/\/admin\/contents\/\d+$/);
		await expect(page.getByRole('heading', { level: 1, name: title })).toBeVisible();
	} finally {
		await removeContentIfPresent(page, title);
	}
});

test('コンテンツ一覧: 行をグループへドラッグすると親が付け替わる', async ({ page }) => {
	const groupTitle = uniqueId('E2EドラッグD先');
	const childTitle = uniqueId('E2EドラッグD元');
	try {
		await createGroupContent(page, groupTitle);
		await createLinkContent(page, childTitle);
		await page.goto('/admin/contents');

		await dragContentRowOnto(page, childTitle, groupTitle);
		// 見える範囲は変わらない (誰でも→誰でも) ので確認ダイアログは出ず、そのまま反映される。
		await expect(
			contentRow(page, childTitle).getByRole('cell', { name: groupTitle })
		).toBeVisible();
	} finally {
		await removeContentIfPresent(page, childTitle);
		await removeContentIfPresent(page, groupTitle);
	}
});

test('コンテンツ一覧: グループ以外・自分の子孫へはドラッグしても親が変わらない', async ({
	page
}) => {
	const parentGroupTitle = uniqueId('E2Eドラッグ親G');
	const childGroupTitle = uniqueId('E2Eドラッグ子G');
	const linkTitle = uniqueId('E2Eドラッグリンク');
	try {
		await createGroupContent(page, parentGroupTitle);
		await createGroupContent(page, childGroupTitle);
		await createLinkContent(page, linkTitle);
		await page.goto('/admin/contents');
		await dragContentRowOnto(page, childGroupTitle, parentGroupTitle);
		await expect(
			contentRow(page, childGroupTitle).getByRole('cell', { name: parentGroupTitle })
		).toBeVisible();

		await page.goto('/admin/contents');
		// group 以外 (link) へはドロップできない。
		await dragContentRowOnto(page, parentGroupTitle, linkTitle);
		await expect(
			contentRow(page, parentGroupTitle).getByRole('cell', { name: '(Root)' })
		).toBeVisible();

		await page.goto('/admin/contents');
		// 親グループを、その子である childGroupTitle へドラッグしても循環になるので変わらない。
		await dragContentRowOnto(page, parentGroupTitle, childGroupTitle);
		await expect(
			contentRow(page, parentGroupTitle).getByRole('cell', { name: '(Root)' })
		).toBeVisible();
	} finally {
		await removeContentIfPresent(page, linkTitle);
		await removeContentIfPresent(page, childGroupTitle);
		await removeContentIfPresent(page, parentGroupTitle);
	}
});

test('コンテンツ一覧: ドラッグで見える範囲が広がるときは確認してから反映する', async ({ page }) => {
	const restrictedGroupTitle = uniqueId('E2Eドラッグ限定G');
	const publicGroupTitle = uniqueId('E2Eドラッグ公開G');
	const linkTitle = uniqueId('E2Eドラッグ緩む');
	try {
		await createGroupContent(page, publicGroupTitle);
		await createGroupContent(page, restrictedGroupTitle);
		await page.goto('/admin/contents');
		await clickRowMenuItem(page, contentRow(page, restrictedGroupTitle), /^編集$|^Edit$/);
		await page.waitForURL(/\/admin\/contents\/\d+$/);
		await page.getByLabel(VISIBILITY_LABEL).click();
		await page.getByRole('option', { name: /要ログイン|Sign-in required/ }).click();
		await saveForm(page);

		// 誰でも公開のリンクを、いったんログイン必須のグループへ入れて絞る (確認は出ない)。
		await createLinkContent(page, linkTitle, { parentTitle: restrictedGroupTitle });
		await page.goto('/admin/contents');
		await expect(
			contentRow(page, linkTitle).getByRole('cell', { name: restrictedGroupTitle })
		).toBeVisible();

		// 誰でも公開のグループへドラッグすると、実際の見える範囲が広がるので確認が要る。
		await dragContentRowOnto(page, linkTitle, publicGroupTitle);
		const confirmDialog = page.getByRole('alertdialog');
		await confirmDialog.waitFor();
		await expect(confirmDialog).toContainText(linkTitle);
		await expect(confirmDialog).toContainText(publicGroupTitle);

		// キャンセルすると親は変わらない。閉じるアニメーションの途中で次のドラッグを始めると
		// 残っている背景が掴む操作を受け止めてしまうため、消え終わるまで待つ。
		await confirmDialog.getByRole('button', { name: CANCEL_BUTTON_NAME }).click();
		await expect(confirmDialog).toHaveCount(0);
		await expect(
			contentRow(page, linkTitle).getByRole('cell', { name: restrictedGroupTitle })
		).toBeVisible();

		// もう一度ドラッグし、今度は保存する。
		await dragContentRowOnto(page, linkTitle, publicGroupTitle);
		await confirmDialog.waitFor();
		await confirmDialog.getByRole('button', { name: SAVE_BUTTON_NAME }).click();
		await expect(confirmDialog).toHaveCount(0);
		await expect(
			contentRow(page, linkTitle).getByRole('cell', { name: publicGroupTitle })
		).toBeVisible();
	} finally {
		await removeContentIfPresent(page, linkTitle);
		await removeContentIfPresent(page, restrictedGroupTitle);
		await removeContentIfPresent(page, publicGroupTitle);
	}
});

test('コンテンツ一覧: 見出しの行へドラッグするとルートへ戻り、見える範囲が広がるときは確認する', async ({
	page
}) => {
	const restrictedGroupTitle = uniqueId('E2Eルート限定G');
	const linkTitle = uniqueId('E2Eルートへ');
	try {
		await createGroupContent(page, restrictedGroupTitle);
		await page.goto('/admin/contents');
		// ルート直下の行を掴んでも、ルートへの落とし先は出ない。
		await startDraggingContentRow(page, restrictedGroupTitle);
		await expect(page.getByTestId('reparent-root-zone')).toHaveCount(0);
		await page.mouse.up();

		await clickRowMenuItem(page, contentRow(page, restrictedGroupTitle), /^編集$|^Edit$/);
		await page.waitForURL(/\/admin\/contents\/\d+$/);
		await page.getByLabel(VISIBILITY_LABEL).click();
		await page.getByRole('option', { name: /要ログイン|Sign-in required/ }).click();
		await saveForm(page);
		await createLinkContent(page, linkTitle, { parentTitle: restrictedGroupTitle });
		await page.goto('/admin/contents');

		// 誰でも公開のリンクをログイン必須のグループから出すと、実際の見える範囲が広がる。
		await dragContentRowToRoot(page, linkTitle);
		const confirmDialog = page.getByRole('alertdialog');
		await confirmDialog.waitFor();
		await expect(confirmDialog).toContainText(linkTitle);
		await confirmDialog.getByRole('button', { name: SAVE_BUTTON_NAME }).click();
		await expect(confirmDialog).toHaveCount(0);
		await expect(contentRow(page, linkTitle).getByRole('cell', { name: '(Root)' })).toBeVisible();
	} finally {
		await removeContentIfPresent(page, linkTitle);
		await removeContentIfPresent(page, restrictedGroupTitle);
	}
});
