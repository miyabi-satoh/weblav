// 画面を Playwright で操作する共通の知識。e2e テストと、仕様書のショット撮影
// (`../scripts/shots.ts`) の両方から読む。
//
// **Node の組み込み以外のランタイムの import を足さないこと**。ショット撮影は
// `node scripts/generate-spec.ts` の型ストリップ実行で動くため、`import type` なら消えるが、
// 値の import はそのまま解決されて test runner を引き込む。
import type { Browser, BrowserContext, Locator, Page } from '@playwright/test';

export function escapeRegExp(value: string): string {
	return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/**
 * FIX: Field.FieldLabelのrequired propが`<span aria-hidden>*</span>`をラベル内に
 * 追加するため、getByLabelは末尾の"*"を含めたラベル文字列でないとマッチしない
 * (getByRoleのaccessible name computationと違い、getByLabelはaria-hiddenの
 * 子要素も含めたラベルのテキスト内容全体でマッチするため)。
 */
export function requiredLabel(...labels: string[]): RegExp {
	return new RegExp(labels.map((label) => `^${escapeRegExp(label)}\\s*\\*?\\s*$`).join('|'));
}

/** 必須の入力欄のラベル。ログイン画面の欄は `*` が付かないので `LOGIN_*` を使う。 */
export const TITLE_LABEL = requiredLabel('タイトル', 'Title');
export const USERNAME_LABEL = requiredLabel('ユーザー名', 'Username');
export const PASSWORD_LABEL = requiredLabel('パスワード', 'Password');
export const PASSWORD_CONFIRM_LABEL = requiredLabel('パスワード(確認)', 'Password (confirm)');
export const CURRENT_PASSWORD_LABEL = requiredLabel('現在のパスワード', 'Current password');

/** ログイン画面の入力欄とボタン。表示言語 (ja/en) のどちらでも見つかるようにする。 */
export const LOGIN_USERNAME_LABEL = /^ユーザー名\s*$|^Username\s*$/;
export const LOGIN_PASSWORD_LABEL = /^パスワード\s*$|^Password\s*$/;
const LOGIN_BUTTON_NAME = /ログイン|Log in/;
export const FORGOT_PASSWORD_LINK_NAME = /^パスワードを忘れた$|^Forgot your password\?$/;

/**
 * パスワードのハッシュ化・照合 (Argon2) を含む要求の結果を待つ時間。
 * Argon2 は重く、マシンが混んでいると既定の5秒を超えるため。
 */
export const PASSWORD_HASH_TIMEOUT = 15_000;

/** フォームの確定と、削除の確認ダイアログの確定。表示言語 (ja/en) のどちらでも見つかるようにする。 */
export const SAVE_BUTTON_NAME = /^保存する$|^Save$/;
const DELETE_CONFIRM_BUTTON_NAME = /削除する|^Delete$/;

/** ダイアログやプレイヤーの「閉じる」。 */
export const CLOSE_BUTTON_NAME = /^閉じる$|^Close$/;

/** ダイアログの「キャンセル」。 */
export const CANCEL_BUTTON_NAME = /^キャンセル$|^Cancel$/;

/** ユーザー一覧の⋮メニューのパスワード再設定。 */
export const RESET_PASSWORD_MENU_ITEM_NAME = /パスワード再設定|Reset password/;

/** ユーザーメニューのリカバリコード。 */
export const RECOVERY_CODE_MENU_ITEM_NAME = /^リカバリコード\.\.\.$|^Recovery code\.\.\.$/;

/** 初回セットアップで、使えないリンクを開いたときの案内。 */
export const INVALID_SETUP_LINK_TEXT = /このリンクは使えません|This link cannot be used/;
/** ログイン画面の、最初の管理者を作る手順への案内。 */
export const SETUP_HELP_LINK_TEXT =
	/最初の管理者をまだ作っていない方はこちら|Haven't created the first administrator yet\?/;

/** リカバリコードを見せる部品 (→ docs/access.md「リカバリコード」) の操作。 */
export const RECOVERY_CODE_SAVE_BUTTON_NAME = /^ファイルに保存$|^Save to file$/;
export const RECOVERY_CODE_KEPT_BUTTON_NAME = /^保管しました$|^I've kept it$/;
export const RECOVERY_CODE_CREATE_BUTTON_NAME = /^作る$|^Create$/;

/** アーカイブの編集ページのタブ「アイテム」。 */
export const ITEMS_TAB_NAME = /^アイテム$|^Items$/;

/** 処理に失敗したことを知らせるダイアログの見出し (`error-dialog.svelte` の既定)。 */
export const ERROR_DIALOG_NAME = /^処理に失敗しました$|^Operation failed$/;

/** サーバーが入力を受け付けなかったとき (422) の文言。 */
export const INVALID_INPUT_TEXT = /入力内容を確認してください|Please check your input/;

/** パスワードの確認欄が一致しないときの文言。 */
export const PASSWORD_MISMATCH_TEXT = /パスワードが一致しません|Passwords do not match/;

/** コンテンツのフォームの公開範囲の欄と、その「本人のみ」の選択肢。 */
export const VISIBILITY_LABEL = /^公開範囲|^Visibility/;
export const VISIBILITY_PRIVATE_OPTION_NAME = /^本人のみ$|^Private$/;

/**
 * 保存できたときに出るトーストの文言 (`admin_settings_saved_toast` など、何を保存したかが前に付く)。
 * トーストの要素のテキストは前後に空白を含むので、末尾の空白を許す。
 */
const SAVED_TEXT = /を保存しました\s*$| saved\.\s*$/;

/** 行の⋮メニューの削除。表示言語 (ja/en) のどちらでも見つかるようにする。 */
export const DELETE_MENU_ITEM_NAME = /削除|Delete/;

/** 管理画面のタブの「コンテンツ」。 */
export const CONTENTS_NAV_LINK_NAME = /^コンテンツ$|^Contents$/;

/** 閲覧画面の下部に出るページ内プレイヤー。 */
export const AUDIO_PLAYER_REGION_NAME = /^オーディオプレイヤー$|^Audio player$/;

/** 閲覧画面の一覧の並べ方の切り替え (リスト・タイル)。 */
export const BROWSE_LAYOUT_LIST_NAME = /^リスト$|^List$/;
export const BROWSE_LAYOUT_TILE_NAME = /^タイル$|^Tiles$/;

/** ページ内プレイヤーの「前の曲」「次の曲」。 */
export const PREVIOUS_TRACK_BUTTON_NAME = /^前の曲$|^Previous track$/;
export const NEXT_TRACK_BUTTON_NAME = /^次の曲$|^Next track$/;
export const NEXT_FILE_BUTTON_NAME = /^次のファイル$|^Next file$/;
/** ページ内プレイヤーの「続けて再生」。 */
export const AUTO_ADVANCE_BUTTON_NAME = /^続けて再生$|^Play next automatically$/;

/**
 * 保存し、保存したことを知らせるトーストが出るまで待つ。
 * 保存ボタンが複数ある画面では押すボタン (`button`) を、どの保存かまで確かめたいときは
 * 待つ文言 (`toastText`) を渡す。省くと、画面に1つの「保存する」と `SAVED_TEXT`。
 * ランタイムの import を足さないため、`expect` ではなく `waitFor` で待つ。
 * 直前の保存のトーストが消えずに残っていても取り違えないよう、押す前からあるトーストに印を付けて除く。
 */
export async function saveForm(
	page: Page,
	options: { button?: Locator; toastText?: RegExp } = {}
): Promise<void> {
	const { button = page.getByRole('button', { name: SAVE_BUTTON_NAME }), toastText = SAVED_TEXT } =
		options;
	await page.evaluate(() => {
		document
			.querySelectorAll('[data-sonner-toast]')
			.forEach((toast) => toast.setAttribute('data-e2e-seen', ''));
	});
	await button.click();
	await page
		.locator('[data-sonner-toast]:not([data-e2e-seen])')
		.filter({ hasText: toastText })
		.waitFor();
}

/** コンテンツの作成ダイアログの2段目の確定ボタン。 */
export const CONTENT_SUBMIT_BUTTON_NAME = /^追加する$|^Add$/;

/** フォルダー・アーカイブのパスを選ぶ。ボタン・ダイアログ・確定の順。 */
export const CHOOSE_PATH_BUTTON_NAME = /^選ぶ\.\.\.$|^Choose\.\.\.$/;
export const DIR_PICKER_NAME = /^フォルダーを選ぶ$|^Choose a folder$/;
export const USE_THIS_FOLDER_BUTTON_NAME = /^このフォルダーにする$|^Use this folder$/;
/** 登録の画面で、選んだパスを選び直すボタン。 */
export const CHANGE_PATH_BUTTON_NAME = /^選び直す\.\.\.$|^Change\.\.\.$/;

/** 登録前の確認ダイアログと、その確定ボタン。 */
export const CONFIRM_REGISTER_NAME = /^この内容で登録しますか$|^Register with these settings\?$/;
export const REGISTER_BUTTON_NAME = /^登録する$|^Register$/;

/** 管理画面の一覧の上にある追加ボタン。表示言語 (ja/en) のどちらでも見つかるようにする。 */
export const CONTENT_ADD_BUTTON_NAME = /^新規追加\.\.\.$|^Add\.\.\.$/;
export const USER_ADD_BUTTON_NAME = /^ユーザーを追加\.\.\.$|^Add user\.\.\.$/;
/** 公開できるフォルダーの行の⋮メニューにある「名前を変更」。 */
export const RENAME_MENU_ITEM_NAME = /^名前を変更\.\.\.$|^Rename\.\.\.$/;
export const ROOT_ADD_BUTTON_NAME = /^フォルダーを追加\.\.\.$|^Add a folder\.\.\.$/;
/** 公開できるフォルダーの追加で、OS のフォルダー選択の窓が閉じるのを待っている間の案内。 */
export const ROOT_PICKING_TEXT =
	/^開いた窓で、公開してよいフォルダーを選んでください。$|^Choose a folder you are willing to share in the window that opened\.$/;

/** コンテンツの作成ダイアログで選ぶ種別のボタン。名前には説明が続くので前方一致で探す。 */
export const CONTENT_TYPE_NAME = {
	link: /^リンク|^Link/,
	file: /^ファイル|^File/,
	folder: /^フォルダー|^Folder/,
	archive: /^アーカイブ|^Archive/,
	group: /^グループ|^Group/
} as const;

/** 追加ボタンを押し、開いたダイアログのフォーカスが落ち着くまで待つ (→ `waitForDialog`)。 */
export async function openAddDialog(
	page: Page,
	name: RegExp,
	options: { timeout?: number } = {}
): Promise<void> {
	await page.getByRole('button', { name }).click();
	await waitForDialog(page, options);
}

/** コンテンツの作成ダイアログで種別を選び、2段目へ進む。 */
export async function chooseContentType(
	page: Page,
	type: keyof typeof CONTENT_TYPE_NAME
): Promise<void> {
	await page.getByRole('dialog').getByRole('button', { name: CONTENT_TYPE_NAME[type] }).click();
}

/**
 * 開いているログイン画面に入力して送信する。`?redirect=` 付きで送られてきた画面でも使える。
 * 成功・失敗のどちらを待つかは呼び出し側が決める。
 */
export async function fillLogInForm(page: Page, username: string, password: string): Promise<void> {
	await page.getByLabel(LOGIN_USERNAME_LABEL).fill(username);
	await page.getByLabel(LOGIN_PASSWORD_LABEL).fill(password);
	await page.getByRole('button', { name: LOGIN_BUTTON_NAME }).click();
}

/** ログイン画面を開いて送信する。成功・失敗のどちらを待つかは呼び出し側が決める。 */
export async function logIn(page: Page, username: string, password: string): Promise<void> {
	await page.goto('/login');
	await fillLogInForm(page, username, password);
}

/**
 * ダイアログが開き、**autofocus が落ち着く**まで待つ。
 *
 * bits-ui の Dialog は開いた直後はダイアログ本体にフォーカスを置き、そのあと
 * 非同期で中の最初の要素へ移す。これを待たずに2つ目の入力欄へ書き込むと、入力の
 * 途中でフォーカスを奪われて文字が最初の欄に流れ込み、2つ目が空のまま残る。
 * パスワード再設定で実際に起き、`fill` を2回呼んだ結果が
 * 「1つ目に2回分が連結・確認欄が空」になって送信がバリデーションで止まっていた。
 *
 * 移り先は入力欄とは限らない (種別を選ぶ Select が先頭にあるフォームでは button)。
 * **ダイアログ本体から中のどれかへ移ったこと**を待つ。
 *
 * `role` はダイアログの種類 (確認ダイアログは `alertdialog`)。
 * フォーカスを動かさないダイアログでは `awaitAutofocus` を `false` にする。
 * `timeout` を省くと Playwright の既定値に従う。
 */
export async function waitForDialog(
	page: Page,
	options: { role?: 'dialog' | 'alertdialog'; awaitAutofocus?: boolean; timeout?: number } = {}
): Promise<void> {
	const { role = 'dialog', awaitAutofocus = true, timeout } = options;
	await page.getByRole(role).waitFor({ timeout });
	if (!awaitAutofocus) return;

	// **最前面のダイアログの中へ**移ったことを見る。
	// - 最初に見つけたものを使うと、入れ子のダイアログ (コンテンツ登録から開く
	//   ディレクトリ選択など) で外側のフォーカスに当たって素通りし、防ぎたい競合が
	//   そのまま残る。ポータルは後から開いたものを後ろに積むので末尾を取る
	// - `data-state="open"` で絞る。bits-ui は閉じるアニメーションが終わるまで
	//   閉じた要素を DOM に残すので、絞らないと末尾が閉じかけの方になりうる
	// - 要素を掴んだまま待つと、ダイアログが作り直されたときに消えた方を見続けて
	//   タイムアウトする。毎回 DOM から取り直す
	await page.waitForFunction(
		(selector) => {
			const dialogs = document.querySelectorAll(selector);
			const dialog = dialogs[dialogs.length - 1];
			if (dialog === undefined) return false;
			const active = document.activeElement;
			return active !== null && active !== dialog && dialog.contains(active);
		},
		`[role="${role}"][data-state="open"]`,
		{ timeout }
	);
}

/**
 * 並列実行するworker同士や同一ミリ秒内の複数実行でも衝突しない識別子を作る
 * (Date.now()だけでは2workerが同一ミリ秒に開始した場合に衝突しうるため)。
 */
export function uniqueId(prefix: string): string {
	return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

/**
 * `testId` の列の値が `text` と完全に一致する行。`getByRole('row', { name })` は行全体の
 * 名前の部分一致で、他の列 (親グループ名など) に同じ文字列を含む行まで拾ってしまうため、
 * 列を限定する。
 */
function rowByCell(page: Page, testId: string, text: string): Locator {
	return page
		.getByRole('row')
		.filter({ has: page.getByTestId(testId).and(page.getByText(text, { exact: true })) });
}

/** ユーザー管理の一覧で、ユーザー名が `username` の行。 */
export function userRow(page: Page, username: string): Locator {
	return rowByCell(page, 'user-username', username);
}

/** コンテンツ管理の一覧で、タイトルが `title` の行。 */
export function contentRow(page: Page, title: string): Locator {
	return rowByCell(page, 'content-title', title);
}

/** 行の⋮メニューを開くボタン。名前には行の名前が入る (→ docs/ui.md「UI 全般」)。 */
export const ROW_ACTIONS_BUTTON_NAME = /の操作$|^Actions for /;

/**
 * コンテンツ管理の一覧で、`title` の行をつまみで掴み、少し動かしてドラッグを始める (離すのは呼び出し側)。
 * Pointer Events を直接使う実装のため (→ `reparent-drag.svelte.ts`)、`dragTo` (HTML5 DnD 前提) ではなく
 * `mouse.move`/`down`/`up` を組み立てて操作する。押した直後・移動の途中・落とす直前と複数回に
 * 分けて動かすのは、1回で飛ばすとドラッグ開始や当たり判定の直前状態を拾えないブラウザーがあるため。
 */
export async function startDraggingContentRow(page: Page, title: string) {
	const handle = contentRow(page, title).getByTestId('reparent-handle');
	await handle.scrollIntoViewIfNeeded();
	const box = await handle.boundingBox();
	if (!box) throw new Error('drag対象の行が見つかりません');
	await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
	await page.mouse.down();
	await page.mouse.move(box.x + 50, box.y + 30, { steps: 5 });
}

/** 行のつまみを掴み、別の行 (グループ) へ落とす。 */
export async function dragContentRowOnto(page: Page, fromTitle: string, toTitle: string) {
	// boundingBox() は自動でスクロールしないため、掴む前に落とす先の行も画面内へ入れる。
	await contentRow(page, toTitle).scrollIntoViewIfNeeded();
	await startDraggingContentRow(page, fromTitle);
	const targetBox = await contentRow(page, toTitle).boundingBox();
	if (!targetBox) throw new Error('drop先の行が見つかりません');
	await page.mouse.move(targetBox.x + targetBox.width / 2, targetBox.y + targetBox.height / 2, {
		steps: 10
	});
	await page.mouse.up();
}

/** 行のつまみを掴み、掴んでいる間だけ見出しの行に出るルートへの落とし先へ落とす。 */
export async function dragContentRowToRoot(page: Page, fromTitle: string) {
	await startDraggingContentRow(page, fromTitle);
	// 落とし先は掴んでから出るので、座標はその後に取る。
	const zoneBox = await page.getByTestId('reparent-root-zone').boundingBox();
	if (!zoneBox) throw new Error('ルートへの落とし先が出ていません');
	await page.mouse.move(zoneBox.x + zoneBox.width / 2, zoneBox.y + zoneBox.height / 2, {
		steps: 10
	});
	await page.mouse.up();
}

/** 開いている削除の確認ダイアログで、削除を確定する。 */
export async function confirmDelete(page: Page): Promise<void> {
	await page
		.getByRole('alertdialog')
		.getByRole('button', { name: DELETE_CONFIRM_BUTTON_NAME })
		.click();
}

/** ログイン状態を持たない storageState。 */
export const EMPTY_STORAGE_STATE = { cookies: [], origins: [] };

/** 未ログインのコンテキスト。呼び出し側が finally で閉じる。 */
export function anonymousContext(browser: Browser): Promise<BrowserContext> {
	return browser.newContext({ storageState: EMPTY_STORAGE_STATE });
}

/**
 * 行の⋮メニューを開き、名前が `name` に一致する項目を押す
 * (行の操作はメニューにまとめてある → docs/ui.md「UI 全般」)。
 */
export async function clickRowMenuItem(page: Page, row: Locator, name: RegExp): Promise<void> {
	await row.getByRole('button', { name: ROW_ACTIONS_BUTTON_NAME }).click();
	await page.getByRole('menu').getByRole('menuitem', { name }).click();
}
