// `just flip-videos` (flip-videos.ts) が撮る筋書き。1つの筋書きが動画1本 (画面の幅ごと) になる。
//
// 筋書きは操作の並びで、1つの操作のあとに1コマ撮る。同じ筋書きを役割ごとに通し、
// 役割で違う所は `roles` で分ける。画面を変えたら、通らなくなった操作をここで直す。

import type { Page } from 'playwright';
import {
	AUDIO_PLAYER_REGION_NAME,
	BROWSE_LAYOUT_LIST_NAME,
	BROWSE_LAYOUT_TILE_NAME,
	CANCEL_BUTTON_NAME,
	CHOOSE_PATH_BUTTON_NAME,
	CLOSE_BUTTON_NAME,
	CONFIRM_REGISTER_NAME,
	CONTENT_ADD_BUTTON_NAME,
	CONTENT_SUBMIT_BUTTON_NAME,
	DELETE_MENU_ITEM_NAME,
	DIR_PICKER_NAME,
	FORGOT_PASSWORD_LINK_NAME,
	LOGIN_USERNAME_LABEL,
	NEXT_TRACK_BUTTON_NAME,
	PASSWORD_CONFIRM_LABEL,
	PASSWORD_HASH_TIMEOUT,
	PASSWORD_LABEL,
	REGISTER_BUTTON_NAME,
	RENAME_MENU_ITEM_NAME,
	RESET_PASSWORD_MENU_ITEM_NAME,
	ROW_ACTIONS_BUTTON_NAME,
	SAVE_BUTTON_NAME,
	TITLE_LABEL,
	USE_THIS_FOLDER_BUTTON_NAME,
	USERNAME_LABEL,
	USER_ADD_BUTTON_NAME,
	VISIBILITY_LABEL,
	chooseContentType,
	clickRowMenuItem,
	confirmDelete,
	contentRow,
	fillLogInForm,
	openAddDialog,
	requiredLabel,
	userRow,
	waitForDialog
} from '../e2e/helpers.ts';
import { waitForPageReady, type ShotFixtures, type ViewportName } from './shots.ts';

export type Role = 'anonymous' | 'user' | 'admin';

export interface StepContext {
	fixtures: ShotFixtures;
	role: Role;
	viewport: ViewportName;
	/** その役割のアカウント。`anonymous` では null。 */
	account: { username: string; password: string } | null;
}

export interface Step {
	/** 動画の帯に出す、この操作の説明。 */
	caption: string;
	run: (page: Page, ctx: StepContext) => Promise<void>;
	/** 省くと、筋書きの役割の全部で通す。 */
	roles?: Role[];
}

export interface Story {
	title: string;
	roles: Role[];
	/** ログインの画面から撮る筋書き。撮る前にログインさせない。 */
	startsLoggedOut?: boolean;
	steps: Step[];
}

const LOGGED_IN: Role[] = ['user', 'admin'];
const ADMIN_ONLY: Role[] = ['admin'];
const USER_ONLY: Role[] = ['user'];

async function open(page: Page, url: string) {
	await page.goto(url);
	await waitForPageReady(page);
}

/** 一覧の縮小画像が読み込まれるまで待つ。 */
async function waitForThumbnails(page: Page) {
	await page.waitForFunction(() => {
		const thumbnails = [...document.querySelectorAll<HTMLImageElement>('img[data-thumbnail-for]')];
		return thumbnails.length > 0 && thumbnails.every((img) => img.complete && img.naturalWidth > 0);
	});
}

/** 開いているダイアログを「キャンセル」で閉じ、消えるまで待つ。 */
async function cancelDialog(page: Page, role: 'dialog' | 'alertdialog' = 'dialog') {
	const dialog = page.getByRole(role);
	await dialog.getByRole('button', { name: CANCEL_BUTTON_NAME }).click();
	await dialog.waitFor({ state: 'hidden' });
}

const openUserMenu = async (page: Page, ctx: StepContext) => {
	await page.getByRole('button', { name: ctx.account!.username, exact: true }).click();
	await page.getByRole('menu').waitFor();
};

const folderUrl = (ctx: StepContext, dir: string) =>
	`/folders/${ctx.fixtures.folderId}?path=${encodeURIComponent(dir)}`;

/** 筋書きの中で作るものの名前。役割ごとに分け、前の役割が作ったものと取り違えない。 */
const ownTitle = (ctx: StepContext) => `動画用のリンク (${ctx.role})`;
const NEW_USERNAME = 'flip-user';

const browse: Story = {
	title: '閲覧',
	roles: ['anonymous', 'user', 'admin'],
	steps: [
		{ caption: 'ホームを開く', run: (page) => open(page, '/') },
		{
			caption: '「ログインした人」のグループの URL を開く (ログインの画面へ送られる)',
			roles: ['anonymous'],
			async run(page, ctx) {
				await page.goto(`/groups/${ctx.fixtures.groupId}`);
				await page.getByLabel(LOGIN_USERNAME_LABEL).waitFor();
			}
		},
		{
			caption: 'グループを開く',
			roles: LOGGED_IN,
			async run(page) {
				await page
					.getByRole('link', { name: /教材\(グループ\)/ })
					.first()
					.click();
				await page.waitForURL(/\/groups\/\d+/);
				await waitForPageReady(page);
			}
		},
		{
			caption: 'グループの中のアーカイブを開く',
			roles: LOGGED_IN,
			async run(page) {
				await page
					.getByRole('link', { name: /英検・共通テスト索引/ })
					.first()
					.click();
				await page.waitForURL(/\/archives\/\d+/);
				await waitForPageReady(page);
			}
		},
		{
			caption: '絞り込みの「年度」を開く',
			roles: LOGGED_IN,
			async run(page) {
				await page.getByRole('button', { name: '年度', exact: true }).click();
				await page.getByRole('option').first().waitFor();
			}
		},
		{
			caption: '「2024」で絞り込む',
			roles: LOGGED_IN,
			async run(page) {
				await page.getByRole('option', { name: /^2024/ }).click();
				await page.waitForURL((url) => url.searchParams.get('年度') === '2024');
			}
		},
		{
			caption: '音声の行を押して再生する',
			roles: LOGGED_IN,
			async run(page) {
				// 表示タイトルは軸から組み立てたもの。音声の行はボタン、PDF の行はリンクなので、ボタンで選ぶ。
				await page
					.getByRole('button', { name: /英検 1級/ })
					.first()
					.click();
				await page.getByRole('region', { name: AUDIO_PLAYER_REGION_NAME }).waitFor();
			}
		},
		{
			caption: 'プレイヤーで次の曲へ進む',
			roles: LOGGED_IN,
			async run(page) {
				await page
					.getByRole('region', { name: AUDIO_PLAYER_REGION_NAME })
					.getByRole('button', { name: NEXT_TRACK_BUTTON_NAME })
					.click();
			}
		},
		{
			caption: '誰でも見られるフォルダーを開く',
			run: (page, ctx) => open(page, `/folders/${ctx.fixtures.folderId}`)
		},
		{
			caption: 'フォルダーの中の「写真」へ進む',
			async run(page) {
				await page.getByRole('link', { name: /写真/ }).first().click();
				await page.waitForURL((url) => url.searchParams.get('path') === '写真');
				await waitForThumbnails(page);
			}
		},
		{
			caption: '表示をタイルに切り替える',
			async run(page) {
				await page.getByRole('radio', { name: BROWSE_LAYOUT_TILE_NAME }).click();
				await waitForThumbnails(page);
			}
		},
		{
			caption: '画像を押して大きく表示する',
			async run(page) {
				await page.getByRole('link', { name: /運動会_閉会式/ }).click();
				await page.waitForFunction(() => {
					const image = document.querySelector<HTMLImageElement>(
						'.pswp__img:not(.pswp__img--placeholder)'
					);
					const background = document.querySelector('.pswp__bg');
					return (
						image?.complete === true &&
						image.naturalWidth > 0 &&
						background !== null &&
						getComputedStyle(background).opacity === '1'
					);
				});
			}
		},
		{
			caption: '画像を閉じ、表示をリストに戻す',
			async run(page) {
				await page.keyboard.press('Escape');
				await page.locator('.pswp').waitFor({ state: 'detached' });
				await page.getByRole('radio', { name: BROWSE_LAYOUT_LIST_NAME }).click();
			}
		},
		{
			caption: '並び順を「新しい順」にする',
			async run(page) {
				await page.getByRole('button', { name: '並べ替え', exact: true }).click();
				await page.getByRole('option', { name: /^新しい順$/ }).click();
				await page.waitForURL((url) => url.searchParams.get('sort') === 'new');
			}
		},
		{
			caption: 'PDF を押してページの上に開く',
			async run(page, ctx) {
				await open(page, folderUrl(ctx, '共通テスト'));
				await page.getByRole('link', { name: /2024_問題\.pdf/ }).click();
				// 描き終えるまでキャンバスは幅を持たない (→ pdf-page.svelte)。
				await page.waitForFunction(
					() => (document.querySelector<HTMLCanvasElement>('canvas')?.width ?? 0) > 0
				);
			}
		},
		{
			caption: '検索の画面で「リスニング」を探す',
			async run(page) {
				await open(page, '/search');
				await page.getByLabel(/^検索する語句/).fill('リスニング');
				await page.keyboard.press('Enter');
				await page.getByRole('heading', { level: 2 }).first().waitFor();
			}
		},
		{ caption: 'マニュアルの目次を開く', run: (page) => open(page, '/help') },
		{ caption: 'マニュアルのページを開く', run: (page) => open(page, '/help/setup') }
	]
};

const account: Story = {
	title: 'ログインとアカウント',
	roles: LOGGED_IN,
	startsLoggedOut: true,
	steps: [
		{
			caption: 'ログインの画面を開く',
			async run(page) {
				await page.goto('/login');
				await waitForPageReady(page);
			}
		},
		{
			caption: '誤ったパスワードでログインする',
			async run(page, ctx) {
				await fillLogInForm(page, ctx.account!.username, 'wrong-password');
				await waitForDialog(page, { role: 'alertdialog', awaitAutofocus: false });
			}
		},
		{
			caption: '正しいパスワードでログインする',
			async run(page, ctx) {
				const dialog = page.getByRole('alertdialog');
				await dialog.getByRole('button', { name: CLOSE_BUTTON_NAME }).click();
				await dialog.waitFor({ state: 'hidden' });
				await fillLogInForm(page, ctx.account!.username, ctx.account!.password);
				await page.waitForURL('/', { timeout: PASSWORD_HASH_TIMEOUT });
				await waitForPageReady(page);
			}
		},
		{ caption: 'ユーザーメニューを開く', run: openUserMenu },
		{
			caption: '「パスワードを変更...」を開く',
			async run(page) {
				await page.getByRole('menuitem', { name: /^パスワードを変更\.\.\.$/ }).click();
				await waitForDialog(page);
			}
		},
		{
			caption: '「ユーザー名を変更...」を開く',
			async run(page, ctx) {
				await cancelDialog(page);
				await openUserMenu(page, ctx);
				await page.getByRole('menuitem', { name: /^ユーザー名を変更\.\.\.$/ }).click();
				await waitForDialog(page);
			}
		},
		{
			caption: '「リカバリコード...」を開く',
			async run(page, ctx) {
				await cancelDialog(page);
				await openUserMenu(page, ctx);
				await page.getByRole('menuitem', { name: /^リカバリコード\.\.\.$/ }).click();
				await waitForDialog(page);
			}
		},
		{
			caption: 'テーマの切り替えを開く',
			async run(page) {
				await cancelDialog(page);
				await page.getByRole('button', { name: /^テーマを切り替え$/ }).click();
				await page.getByRole('menu').waitFor();
			}
		},
		{
			caption: 'テーマを「ダーク」にする',
			async run(page) {
				await page.getByRole('menuitemradio', { name: /^ダーク$/ }).click();
				await page.getByRole('menu').waitFor({ state: 'hidden' });
			}
		},
		{
			caption: 'テーマを「ライト」に戻し、言語の切り替えを開く',
			async run(page) {
				await page.getByRole('button', { name: /^テーマを切り替え$/ }).click();
				await page.getByRole('menuitemradio', { name: /^ライト$/ }).click();
				await page.getByRole('menu').waitFor({ state: 'hidden' });
				await page.getByRole('button', { name: /^言語を切り替え$/ }).click();
				await page.getByRole('menu').waitFor();
			}
		},
		{
			caption: '言語を「English」にする',
			async run(page) {
				await page.getByRole('menuitemradio', { name: /^English$/ }).click();
				await page.getByRole('button', { name: /^Toggle language$/ }).waitFor();
			}
		},
		{
			caption: '言語を「日本語」に戻し、共有のメニューを開く',
			async run(page) {
				await page.getByRole('button', { name: /^Toggle language$/ }).click();
				await page.getByRole('menuitemradio', { name: /^Japanese$/ }).click();
				await page.getByRole('button', { name: /^言語を切り替え$/ }).waitFor();
				await page.getByRole('button', { name: /^このページを共有$/ }).click();
				await page.getByRole('menu').waitFor();
			}
		},
		{
			caption: '「QR コードを表示...」を開く',
			async run(page) {
				await page.getByRole('menuitem', { name: /^QR コードを表示\.\.\.$/ }).click();
				await page.getByRole('dialog').waitFor();
			}
		},
		{
			caption: 'ユーザーメニューからログアウトする',
			async run(page, ctx) {
				await page.keyboard.press('Escape');
				await page.getByRole('dialog').waitFor({ state: 'hidden' });
				await openUserMenu(page, ctx);
				await page.getByRole('menuitem', { name: /^ログアウトする$/ }).click();
				await page.getByRole('link', { name: /^ログイン$/ }).waitFor();
			}
		},
		{
			caption: 'ログインの画面から「パスワードを忘れた」を開く',
			async run(page) {
				await page.goto('/login');
				await page.getByRole('link', { name: FORGOT_PASSWORD_LINK_NAME }).click();
				await page.waitForURL(/\/recover/);
				await waitForPageReady(page);
			}
		}
	]
};

const contents: Story = {
	title: '管理: コンテンツ',
	roles: LOGGED_IN,
	steps: [
		{ caption: 'コンテンツ管理を開く', run: (page) => open(page, '/admin/contents') },
		{
			caption: '「追加...」を押す',
			run: (page) => openAddDialog(page, CONTENT_ADD_BUTTON_NAME)
		},
		{
			caption: '種類で「リンク」を選ぶ',
			async run(page) {
				await chooseContentType(page, 'link');
				await page.getByLabel(requiredLabel('URL')).waitFor();
			}
		},
		{
			caption: 'URL を入れる',
			// サーバーが取りに行かない (公開アドレスではない) URL。タイトルの取得で待たされない。
			run: (page, ctx) =>
				page.getByLabel(requiredLabel('URL')).fill(`http://localhost/flip-${ctx.role}`)
		},
		{
			caption: '「追加する」を押す (編集のページへ移る)',
			async run(page) {
				await page.getByRole('button', { name: CONTENT_SUBMIT_BUTTON_NAME }).click();
				await page.waitForURL(/\/admin\/contents\/\d+$/);
				await page.getByRole('button', { name: SAVE_BUTTON_NAME }).waitFor();
			}
		},
		{
			caption: 'タイトルを入れ、公開範囲を開く',
			async run(page, ctx) {
				await page.getByLabel(TITLE_LABEL).fill(ownTitle(ctx));
				await page.getByLabel(VISIBILITY_LABEL).click();
				await page.getByRole('option').first().waitFor();
			}
		},
		{
			caption: '「ログインした人」を選んで保存する',
			async run(page) {
				await page.getByRole('option', { name: /^ログインした人|^Logged-in users/ }).click();
				await page.getByRole('button', { name: SAVE_BUTTON_NAME }).click();
				await page.getByText(/を保存しました。\s*$/).waitFor();
			}
		},
		{
			caption: '一覧に戻り、作った行の操作のメニューを開く',
			async run(page, ctx) {
				await open(page, '/admin/contents');
				const row = contentRow(page, ownTitle(ctx));
				await row.scrollIntoViewIfNeeded();
				await row.getByRole('button', { name: ROW_ACTIONS_BUTTON_NAME }).click();
				await page.getByRole('menu').waitFor();
			}
		},
		{
			caption: '「削除...」を押す',
			async run(page) {
				await page.getByRole('menu').getByRole('menuitem', { name: DELETE_MENU_ITEM_NAME }).click();
				await waitForDialog(page, { role: 'alertdialog', awaitAutofocus: false });
			}
		},
		{
			caption: '削除する',
			async run(page, ctx) {
				await confirmDelete(page);
				await contentRow(page, ownTitle(ctx)).waitFor({ state: 'detached' });
			}
		},
		{
			caption: '「追加...」から「フォルダー」を選ぶ',
			async run(page) {
				await openAddDialog(page, CONTENT_ADD_BUTTON_NAME);
				await chooseContentType(page, 'folder');
				await page.getByRole('button', { name: CHOOSE_PATH_BUTTON_NAME }).waitFor();
			}
		},
		{
			caption: 'パスの「選ぶ...」を押す (公開できるフォルダーが並ぶ)',
			async run(page) {
				await page.getByRole('button', { name: CHOOSE_PATH_BUTTON_NAME }).click();
				await page.getByRole('dialog', { name: DIR_PICKER_NAME }).waitFor();
			}
		},
		{
			caption: '公開できるフォルダーの中へ進む',
			async run(page, ctx) {
				await page.getByText(ctx.fixtures.sharedFolderName, { exact: true }).click();
				await page.getByText(/^1\sつ上へ$/).waitFor();
			}
		},
		{
			caption: '「教材」へ進み、「このフォルダーにする」を押す',
			async run(page) {
				const picker = page.getByRole('dialog', { name: DIR_PICKER_NAME });
				await picker.getByText('教材', { exact: true }).click();
				await picker.getByText('英検', { exact: true }).waitFor();
				await picker.getByRole('button', { name: USE_THIS_FOLDER_BUTTON_NAME }).click();
				await picker.waitFor({ state: 'hidden' });
			}
		},
		{
			caption: '「追加する」を押す (登録の前の確認が出る)',
			async run(page) {
				await page.getByRole('button', { name: CONTENT_SUBMIT_BUTTON_NAME }).click();
				await page.getByRole('dialog', { name: CONFIRM_REGISTER_NAME }).waitFor();
				await page.waitForFunction(() => !/ファイルを数えています/.test(document.body.innerText));
			}
		},
		{
			caption: '「登録する」を押す (編集のページへ移る)',
			async run(page) {
				await page.getByRole('button', { name: REGISTER_BUTTON_NAME }).click();
				await page.waitForURL(/\/admin\/contents\/\d+$/);
				await page.getByRole('button', { name: SAVE_BUTTON_NAME }).waitFor();
			}
		},
		{
			caption: '管理者が作ったフォルダーの編集のページを開く',
			async run(page, ctx) {
				await open(page, `/admin/contents/${ctx.fixtures.folderId}`);
				await page.getByRole('heading', { level: 1 }).waitFor();
			}
		}
	]
};

const archive: Story = {
	title: '管理: アーカイブ',
	roles: ['admin', 'user'],
	steps: [
		{
			caption: 'アーカイブの編集のページを開く (基本情報)',
			async run(page, ctx) {
				await open(page, `/admin/contents/${ctx.fixtures.archiveId}`);
				await page.getByRole('heading', { level: 1 }).waitFor();
			}
		},
		{
			caption: '「軸」のタブを開く',
			async run(page) {
				await page.getByRole('link', { name: /^軸$/ }).click();
				await page.waitForURL(/\/axes$/);
				await page.getByRole('button', { name: /^級/ }).first().waitFor();
			}
		},
		{
			caption: '軸「級」を選ぶ (値の辞書が出る)',
			async run(page) {
				await page.getByRole('button', { name: /^級/ }).first().click();
				await page.locator('form tbody tr').first().waitFor();
			}
		},
		{
			caption: 'ファイル名によく出る語と、未設定のファイルの例を見る',
			roles: ADMIN_ONLY,
			async run(page) {
				const words = page.getByRole('region', { name: /^ファイル名によく出る単語$/ });
				await words.getByRole('listitem').first().waitFor();
				await page.getByText(/^一致: \d+ \/ 未設定: \d+$/).waitFor();
				await page.getByText(/^未設定のファイルの例$/).click();
				await words.scrollIntoViewIfNeeded();
			}
		},
		{
			caption: '「軸を追加...」を押す',
			roles: ADMIN_ONLY,
			async run(page) {
				await openAddDialog(page, /^軸を追加\.\.\.$/);
				await page.getByRole('dialog').getByRole('radio').first().waitFor();
			}
		},
		{
			caption: '「アイテム」のタブを開く',
			async run(page, ctx) {
				if (ctx.role === 'admin') await cancelDialog(page);
				await page.getByRole('link', { name: /^アイテム$/ }).click();
				await page.waitForURL(/\/items$/);
				await page.getByRole('table').waitFor();
			}
		},
		{
			caption: '「再スキャンする」を押す',
			roles: ADMIN_ONLY,
			async run(page) {
				await page.getByRole('button', { name: /^再スキャンする$/ }).click();
				await page.getByRole('table').waitFor();
			}
		},
		{
			caption: '閲覧の側でアーカイブを開く',
			run: (page, ctx) => open(page, `/archives/${ctx.fixtures.archiveId}`)
		}
	]
};

const admin: Story = {
	title: '管理: ユーザー・公開できるフォルダー・サイト設定',
	roles: ['admin', 'user'],
	steps: [
		{
			caption: 'ユーザー管理を開く',
			roles: ADMIN_ONLY,
			run: (page) => open(page, '/admin/users')
		},
		{
			caption: 'ユーザー管理の URL を開く (コンテンツ管理へ戻される)',
			roles: USER_ONLY,
			async run(page) {
				await page.goto('/admin/users');
				await page.waitForURL(/\/admin\/contents$/);
				await waitForPageReady(page);
			}
		},
		{
			caption: '「ユーザーを追加...」を押す',
			roles: ADMIN_ONLY,
			run: (page) => openAddDialog(page, USER_ADD_BUTTON_NAME)
		},
		{
			caption: 'ユーザー名とパスワードを入れる',
			roles: ADMIN_ONLY,
			async run(page) {
				await page.getByLabel(USERNAME_LABEL).fill(NEW_USERNAME);
				await page.getByLabel(PASSWORD_LABEL).fill('zxcvbnm1');
				await page.getByLabel(PASSWORD_CONFIRM_LABEL).fill('zxcvbnm1');
			}
		},
		{
			caption: '保存する (一覧に行が増える)',
			roles: ADMIN_ONLY,
			async run(page) {
				await page.getByRole('button', { name: SAVE_BUTTON_NAME }).click();
				await userRow(page, NEW_USERNAME).waitFor({ timeout: PASSWORD_HASH_TIMEOUT });
			}
		},
		{
			caption: '行の操作のメニューを開く',
			roles: ADMIN_ONLY,
			async run(page) {
				await page.getByRole('dialog').waitFor({ state: 'hidden' });
				await userRow(page, NEW_USERNAME)
					.getByRole('button', { name: ROW_ACTIONS_BUTTON_NAME })
					.click();
				await page.getByRole('menu').waitFor();
			}
		},
		{
			caption: '「パスワード再設定...」を押す',
			roles: ADMIN_ONLY,
			async run(page) {
				await page
					.getByRole('menu')
					.getByRole('menuitem', { name: RESET_PASSWORD_MENU_ITEM_NAME })
					.click();
				await waitForDialog(page);
			}
		},
		{
			caption: 'キャンセルし、行のメニューから「削除...」を押す',
			roles: ADMIN_ONLY,
			async run(page) {
				await cancelDialog(page);
				await clickRowMenuItem(page, userRow(page, NEW_USERNAME), DELETE_MENU_ITEM_NAME);
				await waitForDialog(page, { role: 'alertdialog', awaitAutofocus: false });
			}
		},
		{
			caption: '削除する',
			roles: ADMIN_ONLY,
			async run(page) {
				await confirmDelete(page);
				await userRow(page, NEW_USERNAME).waitFor({ state: 'detached' });
			}
		},
		{
			caption: '公開できるフォルダーを開く',
			roles: ADMIN_ONLY,
			run: (page) => open(page, '/admin/roots')
		},
		{
			caption: '公開できるフォルダーの URL を開く (コンテンツ管理へ戻される)',
			roles: USER_ONLY,
			async run(page) {
				await page.goto('/admin/roots');
				await page.waitForURL(/\/admin\/contents$/);
				await waitForPageReady(page);
			}
		},
		{
			caption: '行のメニューから「名前を変更...」を押す',
			roles: ADMIN_ONLY,
			async run(page) {
				await clickRowMenuItem(page, page.getByRole('listitem').first(), RENAME_MENU_ITEM_NAME);
				await waitForDialog(page);
			}
		},
		{
			caption: 'キャンセルし、行のメニューから「削除...」を押す (中のコンテンツの件数が出る)',
			roles: ADMIN_ONLY,
			async run(page) {
				await cancelDialog(page);
				await clickRowMenuItem(page, page.getByRole('listitem').first(), DELETE_MENU_ITEM_NAME);
				await waitForDialog(page, { role: 'alertdialog', awaitAutofocus: false });
			}
		},
		{
			caption: 'キャンセルし、サイト設定を開く',
			roles: ADMIN_ONLY,
			async run(page) {
				await cancelDialog(page, 'alertdialog');
				await open(page, '/admin/settings');
			}
		},
		{
			caption: 'サイト設定の URL を開く (コンテンツ管理へ戻される)',
			roles: USER_ONLY,
			async run(page) {
				await page.goto('/admin/settings');
				await page.waitForURL(/\/admin\/contents$/);
				await waitForPageReady(page);
			}
		},
		{
			caption: '「サーバー」のアップロード上限を変える (既定から変えた印が付く)',
			roles: ADMIN_ONLY,
			async run(page) {
				const input = page.getByRole('textbox', { name: /^アップロード上限/ });
				await input.fill('500');
				await page.getByRole('heading', { name: /^サーバー$/ }).scrollIntoViewIfNeeded();
			}
		},
		{
			caption: '「既定値に戻す」を押す',
			roles: ADMIN_ONLY,
			run: (page) => page.getByRole('button', { name: /^既定値に戻す: アップロード上限$/ }).click()
		},
		{
			caption: '「ログ」の「詳細ログ」をオンにする',
			roles: ADMIN_ONLY,
			async run(page) {
				const verbose = page.getByRole('switch', { name: /^詳細ログ$/ });
				await verbose.click();
				await page.getByRole('switch', { name: /^詳細ログ$/, checked: true }).waitFor();
				await page.getByRole('heading', { name: /^ログ$/ }).scrollIntoViewIfNeeded();
			}
		},
		{
			caption: '「バックアップ」「このアプリについて」まで下げる',
			roles: ADMIN_ONLY,
			run: (page) => page.evaluate(() => window.scrollTo(0, document.body.scrollHeight))
		}
	]
};

/** 動画のファイル名の頭になる名前 → 筋書き。 */
export const stories: Record<string, Story> = { browse, account, contents, archive, admin };
