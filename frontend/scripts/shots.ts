// `{{shot:id}}` プレースホルダーに対応する撮影手順のレジストリ。
// 「どうやってその画面状態に到達するか」はコードでしか表現できないため、
// 仕様書の原稿の側にはロジックを書かず、ここに手順として持たせる。
//
// 各エントリの `run` は (page, ctx) => Promise<void> の形。ctx.baseURL は一時起動した
// weblav backend のURL、ctx.outFile は撮影後のスクリーンショット保存先。
// `viewports` を省略すると既定で desktop/tablet/mobile の3サイズを撮り、
// generate-spec.ts 側で横並び比較表として展開する。レスポンシブで見た目が
// 変わらないと分かったショットは `viewports: ['desktop']` のように絞って1枚に縮小できる。

import type { Page } from 'playwright';
import {
	AUDIO_PLAYER_REGION_NAME,
	BROWSE_LAYOUT_LIST_NAME,
	BROWSE_LAYOUT_TILE_NAME,
	CHOOSE_PATH_BUTTON_NAME,
	CONFIRM_REGISTER_NAME,
	CONTENT_ADD_BUTTON_NAME,
	DELETE_MENU_ITEM_NAME,
	DIR_PICKER_NAME,
	RENAME_MENU_ITEM_NAME,
	ROOT_ADD_BUTTON_NAME,
	ROOT_PICKING_TEXT,
	SAVE_BUTTON_NAME,
	VISIBILITY_LABEL,
	USER_ADD_BUTTON_NAME,
	chooseContentType,
	clickRowMenuItem,
	logIn,
	openAddDialog,
	userRow,
	waitForDialog
} from '../e2e/helpers.ts';
import { TEST_ADMIN, TEST_USER } from './test-account.ts';

/** generate-spec.ts が作成したフィクスチャのid(→ 同ファイルの setupArchiveFixtures)。 */
export interface ShotFixtures {
	groupId: number;
	folderId: number;
	archiveId: number;
	fileId: number;
	/** 登録済みの「公開できるフォルダー」の絶対パス (→ docs/folders.md「公開できるフォルダー」)。 */
	sharedFolder: string;
	/** `sharedFolder` の名前。ピッカーの最初の一覧に、フルパスの代わりに出る。 */
	sharedFolderName: string;
}

export interface ShotContext {
	baseURL: string;
	outFile: string;
	fixtures: ShotFixtures;
}

export type ShotFn = (page: Page, ctx: ShotContext) => Promise<void>;

export type ViewportName = 'desktop' | 'tablet' | 'mobile';

export const VIEWPORTS: Record<ViewportName, { width: number; height: number }> = {
	desktop: { width: 1280, height: 800 },
	tablet: { width: 768, height: 1024 },
	mobile: { width: 390, height: 844 }
};

export const DEFAULT_VIEWPORTS: ViewportName[] = ['desktop', 'tablet', 'mobile'];

export interface ShotEntry {
	run: ShotFn;
	/** 省略時は DEFAULT_VIEWPORTS (レスポンシブ比較表になる)。 */
	viewports?: ViewportName[];
}

async function login(page: Page, username: string, password: string) {
	// 先に前のショットのセッションを捨てる。残っているとログインページは
	// ログイン済みと見なして行き先へ送り返し、フォームが描画されない
	// (→ frontend/src/routes/login/+page.ts)。
	await logout(page);
	await logIn(page, username, password);
	await page.waitForURL('/', { timeout: 5000 });
}

/** 管理者でログインしてから画面を開き、共通の描画完了まで待つ。 */
async function openAsAdmin(page: Page, baseURL: string, pathname: string | URL): Promise<void> {
	await login(page, TEST_ADMIN.username, TEST_ADMIN.password);
	await page.goto(new URL(pathname, baseURL).toString());
	await waitForPageReady(page);
}

/**
 * セッションCookieを捨てて未ログイン状態に戻す。
 * ショットは1つの Page を共有するため、前のショットのログインがそのまま残る。
 * ロケール指定の Cookie (`WEBLAV_LOCALE`) だけは撮影言語を保つために復元する。
 */
async function logout(page: Page) {
	const context = page.context();
	const kept = (await context.cookies()).filter((cookie) => cookie.name === 'WEBLAV_LOCALE');
	await context.clearCookies();
	await context.addCookies(kept);
}

/**
 * ナビゲーション直後、他に待つべき要素操作が無いショットの「落ち着き待ち」。
 * `waitUntil: 'networkidle'` は不安定なテストの原因になるとして公式に非推奨
 * (playwright-core `types.d.ts` "DISCOURAGED")なので使わない。かわりに、全ページ
 * 共通のヘッダー(既知要素)の描画とフォント読み込み完了を待つ。個々のページの
 * データはSvelteKitの`load()`がコンポーネントの描画前に解決している(→各+page.ts)
 * ため、ヘッダーが描画された時点でページ本体のデータも揃っている。
 */
async function waitForPageReady(page: Page): Promise<void> {
	await page.getByRole('link', { name: 'WebLAV' }).waitFor({ timeout: 5000 });
	await page.evaluate(() => document.fonts.ready);
}

/** フィクスチャの `写真` フォルダーを開き、行の縮小画像が読み込まれるまで待つ。 */
async function openPhotoFolder(page: Page, ctx: ShotContext): Promise<void> {
	const url = new URL(`/folders/${ctx.fixtures.folderId}`, ctx.baseURL);
	url.searchParams.set('path', '写真');
	await openAsAdmin(page, ctx.baseURL, url);
	await waitForThumbnails(page);
}

/** 一覧の縮小画像が読み込まれるまで待つ。 */
async function waitForThumbnails(page: Page): Promise<void> {
	await page.waitForFunction(
		() => {
			const thumbnails = [
				...document.querySelectorAll<HTMLImageElement>('img[data-thumbnail-for]')
			];
			return (
				thumbnails.length > 0 && thumbnails.every((img) => img.complete && img.naturalWidth > 0)
			);
		},
		null,
		{ timeout: 5000 }
	);
}

export const shots: Record<string, ShotEntry> = {
	// 未ログイン状態でログインページへ直接アクセスした初期状態。
	login_initial: {
		async run(page, ctx) {
			await page.goto(`${ctx.baseURL}/login`);
			await waitForPageReady(page);
			await page.screenshot({ path: ctx.outFile });
		}
	},

	// トークン無しでセットアップページを開き、使えないリンクだと案内された状態。
	// フォームが出た状態は撮れない (仕様書用のサーバーには管理者を入れてある)。
	setup_invalid_link: {
		async run(page, ctx) {
			await page.goto(`${ctx.baseURL}/setup`);
			await waitForPageReady(page);
			await page.screenshot({ path: ctx.outFile });
		},
		viewports: ['desktop']
	},

	// パスワードを誤ってログインし、エラーダイアログが表示された状態。
	login_error: {
		async run(page, ctx) {
			await logIn(page, TEST_ADMIN.username, 'wrong-password');
			await waitForDialog(page, { role: 'alertdialog', awaitAutofocus: false, timeout: 5000 });
			await page.screenshot({ path: ctx.outFile, animations: 'disabled' });
		}
	},

	// ログイン済みでトップページ(コンテンツ一覧)を表示した状態。自分の本人のみの行に印が付く。
	home_with_contents: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, '/');
			await page.screenshot({ path: ctx.outFile });
		}
	},

	// 未ログインでトップページを表示した状態。ヘッダーがログインリンクになり、
	// visibility='public' のコンテンツだけが並ぶ。
	home_anonymous: {
		async run(page, ctx) {
			await logout(page);
			await page.goto(`${ctx.baseURL}/`);
			await waitForPageReady(page);
			await page.screenshot({ path: ctx.outFile });
		}
	},

	// ログイン済みで、ヘッダーのユーザーメニューを開いた状態。
	header_user_menu: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, '/');
			await page.getByRole('button', { name: TEST_ADMIN.username, exact: true }).click();
			await page.getByRole('menu').waitFor({ timeout: 5000 });
			await page.screenshot({ path: ctx.outFile, animations: 'disabled' });
		}
	},

	// ユーザーメニューの「リカバリコード...」を開き、今のパスワードを求めている状態。
	// 作ると admin のコードが替わるので、作る手前で止める。
	recovery_code_dialog: {
		viewports: ['desktop'],
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, '/');
			await page.getByRole('button', { name: TEST_ADMIN.username, exact: true }).click();
			await page.getByRole('menuitem', { name: /^リカバリコード\.\.\.$/ }).click();
			await waitForDialog(page);
			await page.screenshot({ path: ctx.outFile, animations: 'disabled' });
		}
	},

	// 未ログインで、パスワードの再設定ページを開いた状態。
	recover_initial: {
		async run(page, ctx) {
			await logout(page);
			await page.goto(`${ctx.baseURL}/recover`);
			await waitForPageReady(page);
			await page.screenshot({ path: ctx.outFile });
		}
	},

	// userで管理画面を開いた状態。画面を切り替えるタブが出ない。タブの有無だけを
	// 示すショットなので1サイズでよい。
	admin_contents_list_as_user: {
		viewports: ['desktop'],
		async run(page, ctx) {
			await login(page, TEST_USER.username, TEST_USER.password);
			await page.goto(`${ctx.baseURL}/admin/contents`);
			await waitForPageReady(page);
			await page.screenshot({ path: ctx.outFile });
		}
	},

	// 管理画面: コンテンツ一覧。作成者の列に他人 (testuser) と削除されたユーザーも出て、非表示の行も並ぶ。
	admin_contents_list: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, '/admin/contents');
			await page.screenshot({ path: ctx.outFile });
		}
	},

	// 管理画面: 「コンテンツを追加」ダイアログを開いた状態 (1段目の種別の選択)。
	admin_contents_add_dialog: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, '/admin/contents');
			await openAddDialog(page, CONTENT_ADD_BUTTON_NAME, { timeout: 5000 });
			await page.screenshot({ path: ctx.outFile, animations: 'disabled' });
		}
	},

	// 作成ダイアログの2段目。フォルダーを選んだ状態 (公開範囲はフォルダーにだけ出る)。
	admin_contents_add_form: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, '/admin/contents');
			await openAddDialog(page, CONTENT_ADD_BUTTON_NAME, { timeout: 5000 });
			await chooseContentType(page, 'folder');
			await page.getByRole('button', { name: CHOOSE_PATH_BUTTON_NAME }).waitFor({ timeout: 5000 });
			await page.screenshot({ path: ctx.outFile, animations: 'disabled' });
		}
	},

	// ディレクトリ選択ダイアログ。登録済みの「公開できるフォルダー」から1階層降りた状態
	// (パンくずが公開できるフォルダーから始まる)。
	admin_contents_dir_picker: {
		viewports: ['desktop'],
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, '/admin/contents');
			await openAddDialog(page, CONTENT_ADD_BUTTON_NAME, { timeout: 5000 });
			await chooseContentType(page, 'folder');
			await page.getByRole('button', { name: CHOOSE_PATH_BUTTON_NAME }).click();
			await page.getByRole('dialog', { name: DIR_PICKER_NAME }).waitFor({ timeout: 5000 });
			// 最初の一覧は登録済みの「公開できるフォルダー」。その中へ1階層降りる。
			// ボタンの accessible name はアイコンのSVGを含んで完全一致しないため、
			// 行内のテキストで特定する (クリックは親のbuttonへバブルする)。
			await page.getByText(ctx.fixtures.sharedFolderName, { exact: true }).click();
			// 降りた後にだけ出る「上の階層へ」の行で、一覧の切り替わりを待つ。
			await page.getByText(/上の階層へ|Go up one level/).waitFor({ timeout: 5000 });
			await page.screenshot({ path: ctx.outFile, animations: 'disabled' });
		}
	},

	// コンテンツの編集ページ。アーカイブなので「基本情報」「軸定義」「アイテム」のタブが出る。
	admin_contents_edit: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, `/admin/contents/${ctx.fixtures.archiveId}`);
			await page.getByRole('button', { name: SAVE_BUTTON_NAME }).waitFor({ timeout: 5000 });
			await page.screenshot({ path: ctx.outFile, animations: 'disabled' });
		}
	},

	// 登録前の確認ダイアログ。フィクスチャのディレクトリを指す folder の公開範囲を
	// 変えて保存しようとした状態 (件数と実効的な閲覧範囲が出る)。
	admin_contents_confirm_register: {
		viewports: ['desktop'],
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, `/admin/contents/${ctx.fixtures.folderId}`);
			// パスを変えずに公開範囲だけ変えても確認が出ることを示す。
			await page.getByLabel(VISIBILITY_LABEL).click();
			await page.getByRole('option', { name: /要ログイン|Sign-in required/ }).click();
			await page.getByRole('button', { name: SAVE_BUTTON_NAME }).click();
			await page.getByRole('dialog', { name: CONFIRM_REGISTER_NAME }).waitFor({ timeout: 5000 });
			await page.waitForFunction(
				() => !/対象のファイルを数えています|Counting files/.test(document.body.innerText),
				null,
				{ timeout: 10000 }
			);
			await page.screenshot({ path: ctx.outFile, animations: 'disabled' });
		}
	},

	// 管理画面: ユーザー一覧。
	admin_users_list: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, '/admin/users');
			await page.screenshot({ path: ctx.outFile });
		}
	},

	// 管理画面: サイト設定。seed.sql が入れた例の値が入っている。下の「サーバー」の区画まで撮る。
	admin_settings: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, '/admin/settings');
			await page.screenshot({ path: ctx.outFile, fullPage: true });
		}
	},

	// 管理画面: 公開できるフォルダー。seed.sql の登録済みフォルダーが並ぶ。
	admin_roots: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, '/admin/roots');
			await page.screenshot({ path: ctx.outFile });
		}
	},

	// 管理画面: 公開できるフォルダーの追加で、OS のフォルダー選択の窓が閉じるのを待っているところ。
	// 窓はブラウザーの外に出るので、撮るときは窓を開く口の応答を返さずに待たせる。
	admin_roots_picking: {
		viewports: ['desktop', 'mobile'],
		async run(page, ctx) {
			await page.route('**/api/v1/admin/roots/pick', () => {});
			await openAsAdmin(page, ctx.baseURL, '/admin/roots');
			await page.getByRole('button', { name: ROOT_ADD_BUTTON_NAME }).click();
			await page.getByText(ROOT_PICKING_TEXT).waitFor({ timeout: 5000 });
			await page.screenshot({ path: ctx.outFile, animations: 'disabled' });
		}
	},

	// 管理画面: 公開できるフォルダーの名前の変更ダイアログ。行の⋮メニューから開く。
	admin_roots_rename: {
		viewports: ['desktop'],
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, '/admin/roots');
			await clickRowMenuItem(page, page.getByRole('listitem').first(), RENAME_MENU_ITEM_NAME);
			await waitForDialog(page, { timeout: 5000 });
			await page.screenshot({ path: ctx.outFile, animations: 'disabled' });
		}
	},

	// 管理画面: 公開できるフォルダーの削除の確認。中のコンテンツの件数が文中に入る。
	admin_roots_delete_confirm: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, '/admin/roots');
			await clickRowMenuItem(page, page.getByRole('listitem').first(), DELETE_MENU_ITEM_NAME);
			await waitForDialog(page, { role: 'alertdialog', awaitAutofocus: false });
			await page.screenshot({ path: ctx.outFile, animations: 'disabled' });
		}
	},

	// 管理画面: 「ユーザーを追加」ダイアログを開いた状態。
	admin_users_add_dialog: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, '/admin/users');
			await openAddDialog(page, USER_ADD_BUTTON_NAME, { timeout: 5000 });
			await page.screenshot({ path: ctx.outFile, animations: 'disabled' });
		}
	},

	// 管理画面: ユーザー削除の確認ダイアログ。対象ユーザー名と、そのユーザーが作成者の
	// 本人のみの件数が文中に入る。
	admin_users_delete_confirm: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, '/admin/users');
			const row = userRow(page, TEST_USER.username);
			await clickRowMenuItem(page, row, DELETE_MENU_ITEM_NAME);
			await waitForDialog(page, { role: 'alertdialog', awaitAutofocus: false, timeout: 5000 });
			await page.screenshot({ path: ctx.outFile, animations: 'disabled' });
		}
	},

	// フォルダー閲覧: 英検/2024配下に降りた状態(パンくず2階層、音声/PDFが混在する一覧)。
	folder_browse: {
		async run(page, ctx) {
			const url = new URL(`/folders/${ctx.fixtures.folderId}`, ctx.baseURL);
			url.searchParams.set('path', '英検/2024');
			await openAsAdmin(page, ctx.baseURL, url);
			await page.screenshot({ path: ctx.outFile });
		}
	},

	// フォルダー閲覧: 写真の並ぶフォルダー。画像の行は先頭に縮小画像が出る (→ docs/ui.md「画像のプレビュー」)。
	folder_browse_images: {
		async run(page, ctx) {
			await openPhotoFolder(page, ctx);
			await page.screenshot({ path: ctx.outFile });
		}
	},

	// フォルダー閲覧: 写真の並ぶフォルダーをタイルに切り替えた状態 (→ docs/ui.md「UI 全般」)。
	// 並べ方は端末 (viewport ごとの context) に覚えさせるので、撮ったらリストに戻して後のショットに持ち越さない。
	folder_browse_images_tile: {
		async run(page, ctx) {
			await openPhotoFolder(page, ctx);
			await page.getByRole('radio', { name: BROWSE_LAYOUT_TILE_NAME }).click();
			await waitForThumbnails(page);
			// 切り替えの塗りは色の遷移で変わるので、終わってから撮る (途中だと前の側が塗られて写る)。
			await page.waitForFunction(() => document.getAnimations().length === 0, null, {
				timeout: 5000
			});
			await page.screenshot({ path: ctx.outFile });
			await page.getByRole('radio', { name: BROWSE_LAYOUT_LIST_NAME }).click();
		}
	},

	// 画像のビューアー: 写真の行を押し、ページの上に重ねて開いた状態。
	image_viewer: {
		async run(page, ctx) {
			await openPhotoFolder(page, ctx);
			await page.getByRole('link', { name: /運動会_閉会式/ }).click();
			await page.waitForFunction(
				() => {
					const image = document.querySelector<HTMLImageElement>(
						'.pswp__img:not(.pswp__img--placeholder)'
					);
					const background = document.querySelector('.pswp__bg');
					// 開くアニメーションは背景の濃さと同じ長さなので、濃さが最後まで上がれば終わっている。
					return (
						image?.complete === true &&
						image.naturalWidth > 0 &&
						background !== null &&
						getComputedStyle(background).opacity === '1'
					);
				},
				null,
				{ timeout: 5000 }
			);
			await page.screenshot({ path: ctx.outFile });
		}
	},

	// PDF・動画・テキストのビューアー: PDF を開き、1ページ目を描き終えた状態。
	file_viewer: {
		async run(page, ctx) {
			const url = new URL(`/folders/${ctx.fixtures.folderId}`, ctx.baseURL);
			url.searchParams.set('path', '共通テスト');
			await openAsAdmin(page, ctx.baseURL, url);
			await page.getByRole('link', { name: /2024_問題\.pdf/ }).click();
			// 描き終えるまでキャンバスは幅を持たない (→ pdf-page.svelte)。
			await page.waitForFunction(
				() => (document.querySelector<HTMLCanvasElement>('canvas')?.width ?? 0) > 0,
				null,
				{ timeout: 10_000 }
			);
			await page.waitForTimeout(300);
			await page.screenshot({ path: ctx.outFile });
		}
	},

	// グループ閲覧: type=link/folder/archive/file/groupの全種類の行が並ぶ状態。
	group_browse: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, `/groups/${ctx.fixtures.groupId}`);
			await page.screenshot({ path: ctx.outFile });
		}
	},

	// 管理画面: アーカイブの軸定義タブ。「級」軸(filenameWord)を選択し、
	// 値の辞書(照合語リスト)も併せて表示する。
	admin_archive_axes: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, `/admin/contents/${ctx.fixtures.archiveId}/axes`);
			// ボタンの accessible name は軸名+抽出元バッジの2行分になるため前方一致で探す。
			// 上下ボタンの読み上げ名も軸名で始まるので、行の先頭にある軸を選ぶボタンを取る。
			await page.getByRole('button', { name: /^級/ }).first().click();
			// 値の辞書は軸を選んでから取得するので、行が出るまで待つ (待たないと空の表が写る)。
			await page.locator('form tbody tr').first().waitFor({ timeout: 5000 });
			await page.screenshot({ path: ctx.outFile, animations: 'disabled' });
		}
	},

	// 管理画面: ファイル名に含まれる語の軸 (「級」) の値の辞書。表にまだ無い語の候補と、
	// 今の表での一致・未設定の件数が出る。未設定のファイルの例も開いておく。
	admin_archive_axes_word_suggestions: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, `/admin/contents/${ctx.fixtures.archiveId}/axes`);
			await page.getByRole('button', { name: /^級/ }).first().click();
			// 候補と件数は軸を選んでから取るので、どちらも出るまで待つ。
			const words = page.getByRole('region', {
				name: /^ファイル名によく出る語$|^Common words in file names$/
			});
			await words.getByRole('listitem').first().waitFor({ timeout: 5000 });
			await page
				.getByText(/^一致 \d+件 \/ 未設定 \d+件$|^Matched: \d+ \/ Unset: \d+$/)
				.waitFor({ timeout: 5000 });
			await page.getByText(/^未設定のファイルの例$|^Examples of unset files$/).click();
			// 狭い幅では値の辞書が軸の一覧の下に来るので、候補の区画を画面に入れる。
			await words.scrollIntoViewIfNeeded();
			await page.screenshot({ path: ctx.outFile, animations: 'disabled' });
		}
	},

	// 管理画面: アーカイブの軸の追加ダイアログ。フォルダーの階層を、階層ごとの値の例を見て選ぶ。
	admin_archive_axes_add_dialog: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, `/admin/contents/${ctx.fixtures.archiveId}/axes`);
			await openAddDialog(page, /^軸を追加\.\.\.$|^Add axis\.\.\.$/, { timeout: 5000 });
			// 階層ごとの値はダイアログを開いてから取るので、選択肢が出るまで待つ。
			await page.getByRole('dialog').getByRole('radio').first().waitFor({ timeout: 5000 });
			await page.screenshot({ path: ctx.outFile, animations: 'disabled' });
		}
	},

	// 管理画面: アーカイブのアイテム一覧タブ。公開済みアイテムが並ぶ状態。
	admin_archive_items: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, `/admin/contents/${ctx.fixtures.archiveId}/items`);
			// アイテムは load() ではなくマウント後に取るので、waitForPageReady では待てない。表が出るまで待つ。
			await page.getByRole('table').waitFor({ timeout: 5000 });
			await page.screenshot({ path: ctx.outFile, animations: 'disabled' });
		}
	},

	// アーカイブ閲覧: 絞り込み前の全件表示(軸ドロップダウン+導出済みタイトルの一覧)。
	archive_view: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, `/archives/${ctx.fixtures.archiveId}`);
			await page.screenshot({ path: ctx.outFile });
		}
	},

	// アーカイブ閲覧: 音声を押して、下部固定のページ内プレイヤーを出した状態。
	archive_view_player: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, `/archives/${ctx.fixtures.archiveId}`);
			await page
				.getByRole('button', { name: /リスニング/ })
				.first()
				.click();
			await page.getByRole('region', { name: AUDIO_PLAYER_REGION_NAME }).waitFor();
			// 再生中の行の塗りは色の切り替えのアニメーションで付くので、終わりまで早送りして撮る。
			await page.screenshot({ path: ctx.outFile, animations: 'disabled' });
		}
	},

	// 検索: 語で探した結果 (コンテンツとアーカイブのファイルの2区画)。
	search: {
		async run(page, ctx) {
			await openAsAdmin(page, ctx.baseURL, `/search?q=${encodeURIComponent('リスニング')}`);
			await page.getByRole('heading', { level: 2 }).first().waitFor();
			await page.screenshot({ path: ctx.outFile });
		}
	},

	// マニュアル: 目次(匿名で開ける)。
	help_toc: {
		async run(page, ctx) {
			await logout(page);
			await page.goto(`${ctx.baseURL}/help`);
			await waitForPageReady(page);
			await page.screenshot({ path: ctx.outFile });
		}
	},

	// マニュアル: 個別ページ(「設置する」)。目次のハイライトも併せて見える。
	help_page: {
		async run(page, ctx) {
			await logout(page);
			await page.goto(`${ctx.baseURL}/help/setup`);
			await waitForPageReady(page);
			await page.screenshot({ path: ctx.outFile });
		}
	},

	// 第三者のライセンス(匿名で開ける)。本文の開閉は閉じたまま撮る。
	licenses: {
		async run(page, ctx) {
			await logout(page);
			await page.goto(`${ctx.baseURL}/licenses`);
			await waitForPageReady(page);
			await page.screenshot({ path: ctx.outFile });
		}
	}
};
