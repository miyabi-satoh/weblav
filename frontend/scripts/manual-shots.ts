// `just manual-shots` から呼ばれる。マニュアルの画像 (docs/manual/{ロケール}/images) のうち、
// 管理画面・セットアップの画面・閲覧側の一部の画面 (ホーム・ログイン・音声のプレイヤーなど) と仕組みの図を、使い捨ての backend で撮り直して WebP で書き出す (→ docs/help.md)。
// 画面の文言や見た目を変えたら流し直す。名前を渡すと、その画像だけを撮る (`just manual-shots roots-page`)。
// 既定は日本語と英語の両方を撮る。`--lang=en` のように渡すと、その言語だけを撮る。
//
// ここに無い画像 (タスクトレイ・OS の窓) は手で撮っている。
// 撮れるものを足すときは、名前を ADMIN_SHOTS などの一覧に足し、下の撮影の流れに `want('名前')` で足す。
// 画面の文言は frontend/messages の訳から引き、写すデータ (フォルダー名など) は SAMPLE で言語ごとに持つ。
// 日本語のデータは元の画像に写っていた名前に合わせてある。

import {
	existsSync,
	mkdirSync,
	mkdtempSync,
	readFileSync,
	readdirSync,
	realpathSync,
	rmSync,
	utimesSync,
	writeFileSync
} from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { chromium, type Browser, type Locator, type Page } from 'playwright';
import { send } from '../e2e/api-helpers.ts';
import {
	BROWSE_ADD_BUTTON_NAME,
	BROWSE_ADD_MENU_ITEM_NAME,
	CHANGE_PATH_BUTTON_NAME,
	CHOOSE_PATH_BUTTON_NAME,
	CONTENT_ADD_BUTTON_NAME,
	DELETE_MENU_ITEM_NAME,
	DIR_PICKER_NAME,
	LOGIN_PASSWORD_LABEL,
	LOGIN_USERNAME_LABEL,
	PASSWORD_CONFIRM_LABEL,
	PASSWORD_LABEL,
	RESET_PASSWORD_MENU_ITEM_NAME,
	ROOT_ADD_BUTTON_NAME,
	ROW_ACTIONS_BUTTON_NAME,
	TITLE_LABEL,
	USERNAME_LABEL,
	USER_ADD_BUTTON_NAME,
	USE_THIS_FOLDER_BUTTON_NAME,
	browseLink,
	chooseContentType,
	deleteSectionButton,
	escapeRegExp,
	openAddDialog,
	waitForDialog
} from '../e2e/helpers.ts';
import { insertTestAdmin, startBackend, type Backend } from './backend-process.ts';
import { withQuery } from '../src/lib/href.ts';
import { gotoReady, waitForPageReady, waitForThumbnails } from './shots.ts';
import { silentWav } from './silent-wav.ts';
import { TEST_ADMIN } from './test-account.ts';

const REPO_ROOT = path.resolve(import.meta.dirname, '../..');
const DIAGRAM_HTML = path.join(import.meta.dirname, 'manual-shots/how-it-works.html');

/** 撮る言語。マニュアルの訳のある言語 (docs/manual/ の下のフォルダー) と同じ。 */
const LANGS = ['ja', 'en'] as const;
type Lang = (typeof LANGS)[number];

/** 言語ごとの、ブラウザーの言語・写すデータ。 */
const SAMPLE = {
	ja: {
		browserLang: 'ja-JP',
		school: '学校',
		windowsPath: 'C:\\Users\\user\\Documents\\学校',
		photos: '写真',
		music: '音楽',
		documents: '書類',
		videos: '動画',
		teaching: '教材',
		recordings: '録音',
		english: '英語',
		otherFiles: [
			'写真/運動会.jpg',
			'音楽/校歌.mp3',
			'書類/お知らせ.pdf',
			'動画/海.mp4',
			'英検/準2級.pdf',
			'下書き/メモ.txt'
		],
		rounds: ['第1回', '第2回', '第3回'],
		kinds: ['問題', '解答'],
		track: '朝の散歩.mp3',
		group: '旅行の思い出',
		linkTitle: '天気予報',
		notice: '今月の予定',
		/** ホームの一覧の画像の下端にする行。並びは言語で変わるので、言語ごとに決める。 */
		homeLastRow: '書類',
		noticePdf: 'お知らせ.pdf',
		noticeLines: [
			'お知らせ',
			'来月の予定をお知らせします。',
			'詳しくは担当までお問い合わせください。'
		],
		introGroup: 'よく見るサイト',
		introMaterials: '資料',
		introTrack: '朝の散歩.wav',
		introPhotos: ['夕焼けの海', '夜の街', '山の朝', '湖と森', '花畑', '雪の日'],
		groupExample: '学校の資料',
		year: '年度',
		round: '回',
		kind: '種類',
		main: '本編',
		notes: '解説'
	},
	en: {
		browserLang: 'en-US',
		school: 'School',
		windowsPath: 'C:\\Users\\user\\Documents\\School',
		photos: 'Photos',
		music: 'Music',
		documents: 'Documents',
		videos: 'Videos',
		teaching: 'Teaching materials',
		recordings: 'Recordings',
		english: 'English',
		otherFiles: [
			'Photos/Sports day.jpg',
			'Music/School song.mp3',
			'Documents/Notice.pdf',
			'Videos/Sea.mp4',
			'EIKEN/Grade Pre-2.pdf',
			'Drafts/Memo.txt'
		],
		rounds: ['Session 1', 'Session 2', 'Session 3'],
		kinds: ['Questions', 'Answers'],
		track: 'Morning walk.mp3',
		group: 'Travel memories',
		linkTitle: 'Weather forecast',
		notice: "This month's schedule",
		homeLastRow: 'Travel memories',
		noticePdf: 'Notice.pdf',
		noticeLines: [
			'Notice',
			"Here is next month's schedule.",
			'For details, contact the person in charge.'
		],
		introGroup: 'Favorite sites',
		introMaterials: 'Materials',
		introTrack: 'Morning walk.wav',
		introPhotos: [
			'Sunset sea',
			'Night city',
			'Mountain morning',
			'Lake and forest',
			'Flower field',
			'Snowy day'
		],
		groupExample: 'School materials',
		year: 'Year',
		round: 'Session',
		kind: 'Type',
		main: 'Main program',
		notes: 'Notes'
	}
} satisfies Record<Lang, Record<string, string | string[]>>;

/** 撮っている言語。言語ごとに下の流れを1回ずつ回す。 */
let lang: Lang = 'ja';
const data = () => SAMPLE[lang];
const outDir = () => path.join(REPO_ROOT, `docs/manual/${lang}/images`);

const MESSAGES = Object.fromEntries(
	LANGS.map((l) => [
		l,
		JSON.parse(readFileSync(path.join(import.meta.dirname, `../messages/${l}.json`), 'utf8'))
	])
) as Record<Lang, Record<string, unknown>>;

/**
 * 画面の文言。paraglide の関数は vite の外では読めないので、訳の JSON を直に引く。
 * 差し込みのある文言は `{名前}` を置き換え、`\{` `\}` は括弧に戻す。
 */
function t(key: string, params: Record<string, string | number> = {}): string {
	const message = MESSAGES[lang][key];
	if (typeof message !== 'string') throw new Error(`文言の形が文字列ではありません: ${key}`);
	return message
		.replace(/(?<!\\)\{(\w+)\}/g, (_, name: string) => {
			if (!(name in params)) throw new Error(`${key}: ${name} を渡していません`);
			return String(params[name]);
		})
		.replace(/\\([{}])/g, '$1');
}
/** 文言のうち、差し込む値 `{param}` より前の部分。 */
function textBefore(key: string, param: string): string {
	const message = MESSAGES[lang][key];
	if (typeof message !== 'string' || !message.includes(`{${param}}`))
		throw new Error(`${key}: {${param}} がありません`);
	return message.split(`{${param}}`)[0];
}
/** 文言と一致する名前。部分一致で別の要素を拾わないよう、前後を閉じる。 */
const exactly = (text: string) => new RegExp(`^${escapeRegExp(text)}$`);
/** 文言で始まる名前 (必須の印 `*` が後ろに付くラベルなど)。 */
const startsWith = (text: string) => new RegExp(`^${escapeRegExp(text)}`);

/** 撮れる画像。WebLAV の画面か図かで分ける。 */
const ADMIN_SHOTS = [
	'roots-page',
	'roots-menu',
	'users-page',
	'user-menu',
	'user-add',
	'user-edit',
	'user-password',
	'user-delete',
	'header-user-menu',
	'setup-admin-menu',
	'account-page',
	'settings-page',
	'settings-home',
	'settings-server',
	'settings-port',
	'maintenance-backup',
	'settings-log',
	'contents-page',
	'add-menu',
	'row-edit-pen',
	'delete-section',
	'content-delete',
	'register-group-menu',
	'quickstart-add-folder',
	'add-folder-form',
	'add-folder-picker',
	'add-folder-ready',
	'quickstart-roots',
	'add-archive-picker',
	'folder-list-view',
	'axis-add-dir',
	'archive-axes-tab',
	'axis-add-word',
	'archive-axes-template',
	'roots-delete',
	'axis-row-actions',
	'archive-items-tab',
	'share-qr',
	'viewing-player',
	'viewing-filter',
	'viewing-sort',
	'edit-link',
	'edit-visibility',
	'viewing-home',
	'archive-values',
	'add-link',
	'add-group',
	'header-login',
	'header-language',
	'header-theme',
	'login',
	'viewing-viewer'
];
/** 管理者がまだいない backend で撮るもの。 */
const SETUP_SHOTS = ['quickstart-setup'];
const DIAGRAM_SHOTS = ['how-it-works'];
/** 別のデータで撮る、はじめにの画像 (3つの画面を並べたもの)。 */
const INTRO_SHOTS = ['intro-overview'];
const ALL_SHOTS = [...ADMIN_SHOTS, ...SETUP_SHOTS, ...DIAGRAM_SHOTS, ...INTRO_SHOTS];
/** コンテンツを追加の画面を順に進めて撮るもの。 */
const FOLDER_SHOTS = [
	'quickstart-add-folder',
	'add-folder-form',
	'add-folder-picker',
	'add-folder-ready'
];

const args = process.argv.slice(2);
const langArgs = args.filter((arg) => arg.startsWith('--lang='));
/** 撮る言語。既定は全部。 */
const ONLY_LANGS = langArgs.map((arg) => arg.slice('--lang='.length));
/** 撮る画像の名前。既定は全部。 */
const ONLY = new Set(args.filter((arg) => !langArgs.includes(arg)));
const unknown = [...ONLY].filter((name) => !ALL_SHOTS.includes(name));
const unknownLangs = ONLY_LANGS.filter((l) => !(LANGS as readonly string[]).includes(l));
if (unknown.length > 0 || unknownLangs.length > 0) {
	// 名前を打ち間違えたまま、何も撮らずに成功で終えないため。
	console.error(
		`撮れない画像の名前か言語です: ${[...unknown, ...unknownLangs].join(', ')}\n` +
			`撮れるもの: ${ALL_SHOTS.join(', ')}\n言語: ${LANGS.join(', ')}`
	);
	process.exit(1);
}
const langs = LANGS.filter((l) => ONLY_LANGS.length === 0 || ONLY_LANGS.includes(l));
const want = (name: string) => ONLY.size === 0 || ONLY.has(name);

/** 管理画面を撮る幅。表の列が詰まらず、画像が本文の幅に収まる。 */
const DESKTOP_WIDTH = 800;
/** スマートフォン幅の画像 (タブがアイコンになる) を撮る幅。 */
const MOBILE_WIDTH = 450;
/** 音声のプレイヤーを撮る幅。操作の段が3段に分かれる、スマートフォンの幅。 */
const PLAYER_WIDTH = 390;
/** プレイヤーに写す曲の長さ (秒)。元の画像の 6:12 に合わせる。 */
const PLAYER_TRACK_SECONDS = 372;
/** スマートフォンの画面の高さ。ブラウザーのアドレスの欄などを除いた、ページが見える高さ。 */
const PHONE_HEIGHT = 724;
/** ビューアーの画像に写す高さ。上の操作と、左右の前後のボタンまで。 */
const VIEWER_SHOT_HEIGHT = 470;
/** はじめにの画像で鳴らす曲の長さ (秒)。 */
const INTRO_TRACK_SECONDS = 150;
/** はじめにの画像の一覧に出る更新日。撮るたびに変わらないよう固定する。 */
const INTRO_MTIME = new Date('2026-10-21T09:00:00+09:00');
const INTRO_HTML = path.join(import.meta.dirname, 'manual-shots/intro-overview.html');
/** 欄が画面の下にはみ出さない高さ。はみ出すと clip の外が切れる。 */
const VIEWPORT_HEIGHT = 2400;
/** WebP の品質。1枚あたり数十 KB に収まり、文字が滲まない (→ docs/help.md)。 */
const WEBP_QUALITY = 0.85;
/**
 * 画像に写すパス (SAMPLE の windowsPath)。OS で違う画面は Windows のものを載せる (→ docs/help.md) ので、
 * 手元のパスを API の応答の中でこれに置き換える。
 */
const windowsSharedPath = () => data().windowsPath;

/**
 * 言語ごとに起動し直すブラウザー。
 * 時刻の欄などブラウザーが描く部品は、コンテキストの locale でなく起動時の言語で書式が決まる。
 * 日本語の画面の画像に「03:00 AM」と写らないよう、撮る言語で起動する。
 */
// 言語ごとの流れの頭で launchBrowser が入れる。
let browser!: Browser;
async function launchBrowser() {
	// Ctrl-C はこちらで受けて片付ける (下の SIGINT)。Playwright に先にブラウザーを閉じさせない。
	browser = await chromium.launch({
		handleSIGINT: false,
		args: [`--lang=${data().browserLang}`]
	});
}
/** makeSharedDir が作った一時フォルダー。最後に消す。 */
const tempDirs: string[] = [];

/** 共有にする「学校」フォルダーと、その中のフォルダー・ファイルを作り、絶対パスを返す。 */
function makeSharedDir(): string {
	const d = data();
	const base = mkdtempSync(path.join(tmpdir(), 'weblav-manual-shots-files-'));
	tempDirs.push(base);
	const shared = path.join(base, d.school);
	const files = [...d.otherFiles];
	for (const year of ['2024', '2025'])
		for (const round of d.rounds)
			for (const kind of d.kinds) files.push(`${d.recordings}/${year}/${round}/${kind}.mp3`);
	for (const rel of files) {
		const file = path.join(shared, rel);
		mkdirSync(path.dirname(file), { recursive: true });
		writeFileSync(file, '');
	}
	// プレイヤーを撮る曲は、鳴らせる中身が要る。名前は元の画像に合わせて .mp3 にし、中身は無音の WAV にする
	// (ブラウザーは拡張子でなく中身で形式を見て鳴らす)。
	writeFileSync(path.join(shared, d.music, d.track), silentWav(PLAYER_TRACK_SECONDS));
	mkdirSync(path.join(shared, d.teaching));
	// サーバーは実パスで返す (macOS の一時フォルダーは /private/var/...)。置き換えはこちらに合わせる。
	return realpathSync(shared);
}

/**
 * 管理者でログインしたページを開く。ログイン画面は撮らないので API で入る。
 * `anonymous` なら、ログインしていない人の画面 (編集のペンなどが出ない) にする。
 */
async function newPage(
	backend: Backend,
	width: number,
	sharedPath?: string,
	{ anonymous = false, height = VIEWPORT_HEIGHT } = {}
): Promise<Page> {
	const context = await browser.newContext({
		viewport: { width, height },
		deviceScaleFactor: 2, // 画面は2倍で撮る (→ docs/help.md)
		baseURL: backend.baseURL,
		locale: data().browserLang
	});
	// 画面の表示言語は cookie で決まる (→ vite.config.ts の strategy)。
	await context.addCookies([{ name: 'WEBLAV_LOCALE', value: lang, url: backend.baseURL }]);
	const page = await context.newPage();
	if (anonymous) return page;
	await send(page.request, 'post', '/auth/login', {
		username: TEST_ADMIN.username,
		password: TEST_ADMIN.password
	});
	if (sharedPath) {
		const local = JSON.stringify(sharedPath).slice(1, -1);
		const windows = JSON.stringify(windowsSharedPath()).slice(1, -1);
		await page.route('**/api/v1/admin/roots', async (route) => {
			if (route.request().method() !== 'GET') return route.continue();
			const res = await route.fetch();
			await route.fulfill({ response: res, body: (await res.text()).replaceAll(local, windows) });
		});
	}
	return page;
}

/** 起動したまま、まだ止めていない backend。中断されたときにまとめて止める。 */
const liveBackends = new Set<Backend>();
/** backend を起動している最中か。その間の Ctrl-C は backend-process.ts が片付けて終える。 */
let starting = false;

async function startWithAdmin(): Promise<Backend> {
	starting = true;
	let backend: Backend;
	try {
		backend = await startBackend('weblav-manual-shots-');
	} finally {
		starting = false;
	}
	liveBackends.add(backend);
	// 管理者を入れられなかったら、呼び出し側の finally に届かないので、ここで止める。
	try {
		insertTestAdmin(backend.dbPath);
	} catch (err) {
		await stopBackend(backend);
		throw err;
	}
	return backend;
}

/** 撮り終えた backend を止める。差し替えた応答が止めた backend へ問い合わせないよう、先にページを閉じる。 */
async function closePagesAndStop(backend: Backend) {
	await Promise.allSettled(browser.contexts().map((context) => context.close()));
	await stopBackend(backend);
}

async function stopBackend(backend: Backend) {
	liveBackends.delete(backend);
	await backend.stop();
}

/** PNG を、ブラウザーの WebP エンコーダーで変換して書き出す。cwebp などを入れずに済む。 */
async function save(page: Page, name: string, png: Buffer) {
	const encoder = await page.context().newPage();
	const webp = await encoder.evaluate(
		async ({ base64, quality }) => {
			const img = new Image();
			img.src = `data:image/png;base64,${base64}`;
			await img.decode();
			const canvas = document.createElement('canvas');
			canvas.width = img.naturalWidth;
			canvas.height = img.naturalHeight;
			canvas.getContext('2d')!.drawImage(img, 0, 0);
			return canvas.toDataURL('image/webp', quality).split(',')[1];
		},
		{ base64: png.toString('base64'), quality: WEBP_QUALITY }
	);
	await encoder.close();
	mkdirSync(outDir(), { recursive: true });
	writeFileSync(path.join(outDir(), `${name}.webp`), Buffer.from(webp, 'base64'));
	console.log(`[shot] ${lang}/${name}`);
}

/** 要素の位置。画面の文言を変えて要素が見つからなくなったら、どの画像のどの要素かを出して止める。 */
async function box(name: string, locator: Locator) {
	const found = await locator.boundingBox().catch((err: Error) => {
		throw new Error(`${name}: 要素が見つかりません (${locator}): ${err.message}`);
	});
	if (!found) throw new Error(`${name}: 要素が見えていません (${locator})`);
	return found;
}

/** 上端の要素から下端の要素までを、ページの横幅いっぱいに撮る。 */
async function shotBetween(
	page: Page,
	name: string,
	top: Locator,
	bottom: Locator,
	{ padTop = 0, padBottom = 0 } = {}
) {
	const a = await box(name, top);
	const b = await box(name, bottom);
	const y = a.y - padTop;
	const width = page.viewportSize()!.width;
	await save(
		page,
		name,
		await page.screenshot({
			animations: 'disabled',
			clip: { x: 0, y, width, height: b.y + b.height + padBottom - y }
		})
	);
}

/**
 * 欄 (section) を、左右と下に余白を付けて撮る。上は欄の区切り線を入れないよう、見出しの少し上から。
 * `bottom` を渡すと、その要素を囲む枠の下端までにする (欄の下の区切り線を入れない)。
 * 枠が無く、すぐ下に次の項目が続くときは `bottomGap` を負にして、次の項目の頭が写らないようにする。
 * `top` を渡すと、見出しでなくその要素の少し上から撮る (欄の途中の項目だけを見せる)。
 */
async function shotSection(
	page: Page,
	name: string,
	heading: Locator,
	bottom?: Locator,
	{ bottomGap = 16, top }: { bottomGap?: number; top?: Locator } = {}
) {
	const PAD = 24;
	await box(name, heading); // 見出しが無ければ、画像の名前を出して止める
	await heading.evaluate((el) => el.scrollIntoView({ block: 'start' }));
	await page.evaluate(() => window.scrollBy(0, -40));
	const section = page.locator('section, form').filter({ has: heading }).last();
	const s = await box(name, section);
	const h = await box(name, top ?? heading);
	const b = bottom ? await box(name, bottom) : undefined;
	const end = b ? b.y + b.height + bottomGap : s.y + s.height;
	const y = h.y - 20;
	await save(
		page,
		name,
		await page.screenshot({
			animations: 'disabled',
			clip: { x: s.x - PAD, y, width: s.width + PAD * 2, height: end + PAD - y }
		})
	);
}

/** いくつかの要素を囲む範囲を、周りに余白を付けて撮る。ヘッダーの一部やフォームの一部だけを見せるときに使う。 */
async function shotAround(page: Page, name: string, locators: Locator[], pad = 16) {
	const boxes = await Promise.all(locators.map((locator) => box(name, locator)));
	const left = Math.max(0, Math.min(...boxes.map((b) => b.x)) - pad);
	const top = Math.max(0, Math.min(...boxes.map((b) => b.y)) - pad);
	const right = Math.min(
		page.viewportSize()!.width,
		Math.max(...boxes.map((b) => b.x + b.width)) + pad
	);
	const bottom = Math.max(...boxes.map((b) => b.y + b.height)) + pad;
	await page.mouse.move(1, 1);
	await save(
		page,
		name,
		await page.screenshot({
			animations: 'disabled',
			clip: { x: left, y: top, width: right - left, height: bottom - top }
		})
	);
}

/** ダイアログなどの要素だけを撮る。ボタンにホバーの色が付かないよう、ポインターを外してから。 */
async function shotElement(page: Page, name: string, locator: Locator) {
	await box(name, locator); // 要素が無ければ、画像の名前を出して止める
	await page.mouse.move(1, 1);
	await save(page, name, await locator.screenshot({ animations: 'disabled' }));
}

const adminNav = (page: Page) =>
	page
		.getByRole('navigation')
		.filter({ hasText: t('admin_settings_nav_title') })
		.last();
const row = (page: Page, title: string) => page.getByRole('row').filter({ hasText: title }).first();
const dialog = (page: Page) => page.getByRole('dialog').last();
/** 登録の画面の上に重なって開く、フォルダーを選ぶ画面。 */
const picker = (page: Page) => page.getByRole('dialog', { name: DIR_PICKER_NAME });

/** ユーザーの章とユーザー名のメニューの画像を撮る。一覧に管理者と一般ユーザーの両方を写すため、先に足す。 */
async function shootUsers(page: Page) {
	const SHOT_USER_PASSWORD = 'E2eSmoke!2026Test';
	let editId = 0;
	for (const [username, role] of [
		['sato', 'admin'],
		['suzuki', 'user'],
		['tanaka', 'user']
	]) {
		const created = (await send(page.request, 'post', '/admin/users', {
			username,
			password: SHOT_USER_PASSWORD,
			role
		})) as { id: number };
		editId = created.id;
	}

	if (want('users-page')) {
		await gotoReady(page, '/admin/users');
		await shotBetween(page, 'users-page', adminNav(page), row(page, 'tanaka'), { padBottom: 16 });
	}
	if (want('user-menu')) {
		await gotoReady(page, '/admin/users');
		await row(page, 'tanaka').getByRole('button', { name: ROW_ACTIONS_BUTTON_NAME }).click();
		await page.getByRole('menu').waitFor();
		await shotBetween(page, 'user-menu', row(page, 'tanaka'), page.getByRole('menu'), {
			padTop: 8,
			padBottom: 16
		});
		await page.keyboard.press('Escape');
		await page.getByRole('menu').waitFor({ state: 'hidden' });
	}
	if (want('user-add')) {
		await gotoReady(page, '/admin/users');
		await openAddDialog(page, USER_ADD_BUTTON_NAME);
		await page.getByLabel(USERNAME_LABEL).fill('yamada');
		await page.getByLabel(PASSWORD_LABEL).fill(SHOT_USER_PASSWORD);
		await page.getByLabel(PASSWORD_CONFIRM_LABEL).fill(SHOT_USER_PASSWORD);
		await shotElement(page, 'user-add', dialog(page));
		await page.keyboard.press('Escape');
	}
	if (want('user-edit')) {
		await gotoReady(page, `/admin/users/${editId}`);
		await shotBetween(
			page,
			'user-edit',
			adminNav(page),
			page.getByRole('button', { name: exactly(t('action_save')) }).last(),
			{ padBottom: 24 }
		);
	}
	if (want('user-password')) {
		// 開いたままの欄を写す (開くと最初の欄にフォーカスが移る)。保存はしない。
		await gotoReady(page, `/admin/users/${editId}`);
		await page.getByRole('button', { name: RESET_PASSWORD_MENU_ITEM_NAME }).click();
		await waitForDialog(page);
		await shotElement(page, 'user-password', dialog(page));
		await page.keyboard.press('Escape');
		await dialog(page).waitFor({ state: 'hidden' });
	}
	if (want('user-delete')) {
		// 確かめの窓を開くだけで、消さない。後の画像にも tanaka を写すため。
		await gotoReady(page, '/admin/users');
		await row(page, 'tanaka').getByRole('button', { name: ROW_ACTIONS_BUTTON_NAME }).click();
		await page.getByRole('menuitem', { name: DELETE_MENU_ITEM_NAME }).click();
		await page.getByRole('alertdialog').waitFor();
		await shotElement(page, 'user-delete', page.getByRole('alertdialog'));
		await page.keyboard.press('Escape');
		await page.getByRole('alertdialog').waitFor({ state: 'hidden' });
	}
	// どちらもユーザー名のメニューを写す。セットアップの直後と、ふだんのログインの章で同じものを見せる。
	for (const name of ['header-user-menu', 'setup-admin-menu']) {
		if (!want(name)) continue;
		await gotoReady(page, '/');
		await page.getByRole('banner').getByRole('button', { name: TEST_ADMIN.username }).click();
		await page.getByRole('menu').waitFor();
		await shotBetween(page, name, page.getByRole('banner'), page.getByRole('menu'), {
			padBottom: 16
		});
		await page.keyboard.press('Escape');
		await page.getByRole('menu').waitFor({ state: 'hidden' });
	}
	if (want('account-page')) {
		await gotoReady(page, '/account');
		// この幅では区画が1列に並ぶので、最後のリカバリコードの区画の下端まで写す。
		await shotBetween(
			page,
			'account-page',
			page.getByRole('heading', { level: 1, name: exactly(t('account_title')) }),
			page.getByRole('region', { name: exactly(t('recovery_code_label')) }),
			{ padTop: 16, padBottom: 16 }
		);
	}
}

/** WebLAV の画面を撮る。コンテンツを足しながら、元の画像と同じ件数の時点で撮る。 */
async function shootAdmin() {
	const backend = await startWithAdmin();
	try {
		const shared = makeSharedDir();
		const page = await newPage(backend, DESKTOP_WIDTH, shared);
		await send(page.request, 'post', '/admin/roots', { path: shared });

		if (want('roots-page')) {
			await gotoReady(page, '/admin/roots');
			await shotBetween(
				page,
				'roots-page',
				adminNav(page),
				page.getByRole('button', { name: ROOT_ADD_BUTTON_NAME }),
				{ padBottom: 16 }
			);
		}
		if (want('roots-menu')) {
			await gotoReady(page, '/admin/roots');
			const actions = page.getByRole('button', { name: ROW_ACTIONS_BUTTON_NAME }).first();
			await actions.click();
			await page.getByRole('menu').waitFor();
			await shotBetween(page, 'roots-menu', actions, page.getByRole('menu'), {
				padTop: 24,
				padBottom: 16
			});
			await page.keyboard.press('Escape');
			await page.getByRole('menu').waitFor({ state: 'hidden' });
		}

		await shootUsers(page);

		const folderIds = new Map<string, number>();
		for (const title of [data().photos, data().music, data().documents]) {
			const folder = (await send(page.request, 'post', '/contents', {
				type: 'folder',
				path: path.join(shared, title),
				visibility: 'public'
			})) as { id: number };
			folderIds.set(title, folder.id);
		}

		if (want('viewing-player')) {
			const phone = await newPage(backend, PLAYER_WIDTH);
			await gotoReady(phone, `/folders/${folderIds.get(data().music)}`);
			await phone.getByRole('button', { name: data().track, exact: true }).click();
			const player = phone.getByRole('region', { name: t('audio_player_region_label') });
			// 鳴っている途中の位置で止め、写る時刻と再生ボタンを決まった形にする。
			await phone.waitForFunction(() => (document.querySelector('audio')?.currentTime ?? 0) > 0);
			await phone.evaluate(() => {
				const audio = document.querySelector('audio')!;
				audio.pause();
				audio.currentTime = 1;
			});
			await player.getByText('0:01').waitFor();
			await shotElement(phone, 'viewing-player', player);
			await phone.context().close();
		}
		const group = (await send(page.request, 'post', '/contents', {
			type: 'group',
			title: data().group,
			visibility: 'public'
		})) as { id: number };
		await send(page.request, 'post', '/contents', {
			type: 'folder',
			path: path.join(shared, data().videos),
			parentId: group.id,
			visibility: 'public'
		});

		const SETTINGS_SHOTS = [
			'settings-page',
			'settings-home',
			'settings-server',
			'settings-port',
			'maintenance-backup',
			'settings-log'
		];
		if (SETTINGS_SHOTS.some(want)) {
			// 使い捨ての backend は空きポートで動く。画像には、入れた直後の値 (既定の値) を写す。
			await page.route('**/api/v1/admin/server-settings', async (route) => {
				const res = await route.fetch();
				const body = await res.json();
				await route.fulfill({
					response: res,
					json: { ...body, saved: body.defaults, running: body.defaults }
				});
			});
			// 使い捨ての backend はログを標準出力に出すので、ダウンロードのボタンが出ない。
			// 入れた直後と同じく、ファイルに書いている答えにする。
			await page.route('**/api/v1/admin/log-settings', async (route) => {
				if (route.request().method() !== 'GET') return route.continue();
				const res = await route.fetch();
				await route.fulfill({ response: res, json: { ...(await res.json()), fileOutput: true } });
			});
			await gotoReady(page, '/admin/settings');
			if (want('settings-page'))
				await shotBetween(
					page,
					'settings-page',
					adminNav(page),
					page.getByRole('heading', { name: t('admin_settings_nav_title'), level: 1 }),
					{ padBottom: 24 }
				);
			if (want('settings-home'))
				await shotSection(
					page,
					'settings-home',
					page.getByRole('heading', { name: t('admin_settings_home_section_heading') })
				);
			const serverHeading = page.getByRole('heading', { name: t('admin_settings_server_heading') });
			if (want('settings-server')) await shotSection(page, 'settings-server', serverHeading);
			if (want('settings-port')) {
				// 保存はしない。変えた項目に付く印と、戻すボタンも写す。
				const port = page.getByRole('textbox', { name: t('admin_settings_port_label') });
				await port.fill('8390');
				await port.blur();
				await shotSection(
					page,
					'settings-port',
					serverHeading,
					page.getByRole('button', {
						name: t('admin_settings_reset_button_label', { name: t('admin_settings_port_label') })
					}),
					// 下に続く説明文の頭が切れて写らないよう、余白 (PAD) を詰める。
					{ bottomGap: -12 }
				);
			}
			if (want('maintenance-backup'))
				await shotSection(
					page,
					'maintenance-backup',
					page.getByRole('heading', { name: t('backup_heading') })
				);
			if (want('settings-log'))
				await shotSection(
					page,
					'settings-log',
					page.getByRole('heading', { name: t('admin_settings_log_heading'), exact: true })
				);
		}

		await gotoReady(page, '/admin/contents');
		if (want('contents-page'))
			await shotBetween(page, 'contents-page', adminNav(page), row(page, data().documents));
		// 閲覧の画面から足す・直す・消す道 (→ 06-register-content.md)。管理者でなくても同じ所に出る。
		if (want('add-menu')) {
			await gotoReady(page, '/');
			await page.getByRole('button', { name: BROWSE_ADD_BUTTON_NAME }).click();
			await page.getByRole('menu').waitFor();
			await shotBetween(page, 'add-menu', page.getByRole('banner'), page.getByRole('menu'), {
				padBottom: 16
			});
			await page.keyboard.press('Escape');
			await page.getByRole('menu').waitFor({ state: 'hidden' });
		}
		const editPen = (page: Page) =>
			page.getByRole('link', { name: t('browse_heading_edit_label', { title: data().photos }) });
		if (want('row-edit-pen')) {
			await gotoReady(page, '/');
			const rowLink = browseLink(page, data().photos);
			await shotBetween(page, 'row-edit-pen', rowLink, editPen(page), {
				padTop: 16,
				padBottom: 16
			});
		}
		if (want('delete-section')) {
			await gotoReady(page, '/');
			await editPen(page).click();
			await page.waitForURL(/\/contents\/\d+$/);
			await waitForPageReady(page);
			await shotSection(
				page,
				'delete-section',
				page.getByRole('heading', { name: exactly(t('common_delete_section_title')) })
			);
		}
		if (want('content-delete')) {
			// 確かめの窓を開くだけで、消さない。
			await gotoReady(page, '/');
			await editPen(page).click();
			await page.waitForURL(/\/contents\/\d+$/);
			await waitForPageReady(page);
			await deleteSectionButton(page).click();
			await page.getByRole('alertdialog').waitFor();
			await shotElement(page, 'content-delete', page.getByRole('alertdialog'));
			await page.keyboard.press('Escape');
			await page.getByRole('alertdialog').waitFor({ state: 'hidden' });
		}
		// ここからはコンテンツの一覧で撮る。上の画像は閲覧の画面と編集ページに移っている。
		if (want('register-group-menu') || FOLDER_SHOTS.some(want))
			await gotoReady(page, '/admin/contents');
		if (want('register-group-menu')) {
			await row(page, data().group).getByRole('button', { name: ROW_ACTIONS_BUTTON_NAME }).click();
			await page.getByRole('menu').waitFor();
			// 下端はメニューに合わせる。行に合わせると、項目の数しだいで下の項目が切れるため。
			await shotBetween(
				page,
				'register-group-menu',
				row(page, data().documents),
				page.getByRole('menu'),
				{
					padBottom: 16
				}
			);
			await page.keyboard.press('Escape');
			// 閉じるのを待つ。次のメニューを待つときに、閉じかけのこのメニューを拾わないため。
			await page.getByRole('menu').waitFor({ state: 'hidden' });
		}
		if (FOLDER_SHOTS.some(want)) {
			await openAddDialog(page, CONTENT_ADD_BUTTON_NAME);
			if (want('quickstart-add-folder'))
				await shotElement(page, 'quickstart-add-folder', dialog(page));
			await chooseContentType(page, 'folder');
			await dialog(page).getByRole('button', { name: CHOOSE_PATH_BUTTON_NAME }).waitFor();
			if (want('add-folder-form')) await shotElement(page, 'add-folder-form', dialog(page));
			await dialog(page).getByRole('button', { name: CHOOSE_PATH_BUTTON_NAME }).click();
			await picker(page).waitFor();
			await dialog(page).getByRole('button', { name: data().school }).click();
			await dialog(page).getByRole('button', { name: data().teaching }).click();
			await dialog(page)
				.getByText(`${data().school} / ${data().teaching}`, { exact: true })
				.last()
				.waitFor();
			if (want('add-folder-picker')) await shotElement(page, 'add-folder-picker', dialog(page));
			await dialog(page).getByRole('button', { name: USE_THIS_FOLDER_BUTTON_NAME }).click();
			await picker(page).waitFor({ state: 'hidden' });
			await dialog(page).getByRole('button', { name: CHANGE_PATH_BUTTON_NAME }).waitFor();
			if (want('add-folder-ready')) await shotElement(page, 'add-folder-ready', dialog(page));
		}

		// 共有の中のコンテンツは、写真・音楽・書類・動画の4件。
		if (want('quickstart-roots')) {
			const narrow = await newPage(backend, MOBILE_WIDTH, shared);
			await gotoReady(narrow, '/admin/roots');
			const rootRow = narrow.getByText(windowsSharedPath()).locator('xpath=ancestor::tr[1]');
			await shotBetween(narrow, 'quickstart-roots', adminNav(narrow), rootRow);
			await narrow.context().close();
		}

		if (want('add-archive-picker')) {
			await gotoReady(page, '/admin/contents');
			await openAddDialog(page, CONTENT_ADD_BUTTON_NAME);
			await chooseContentType(page, 'folder');
			await dialog(page).getByRole('button', { name: CHOOSE_PATH_BUTTON_NAME }).click();
			await picker(page).waitFor();
			await dialog(page).getByRole('button', { name: data().school }).click();
			await dialog(page).getByRole('button', { name: data().recordings }).click();
			await dialog(page).getByRole('button', { name: '2025' }).waitFor();
			await shotElement(page, 'add-archive-picker', dialog(page));
		}

		// マニュアルの手順 (06-archive-setup.md) と同じく、フォルダーを作ってから一覧の表示を使う。
		const created = (await send(page.request, 'post', '/contents', {
			type: 'folder',
			title: data().recordings,
			path: path.join(shared, data().recordings),
			visibility: 'public'
		})) as { id: number };
		// 完全上書き型の PUT なので、作成時の応答をそのまま送り直す。
		const archive = (await send(page.request, 'put', `/contents/${created.id}`, {
			...created,
			listView: true,
			includePatterns: '*.mp3'
		})) as { id: number };
		// 階層ごとの値は、索引を作ってから出る。
		await send(page.request, 'post', `/contents/${archive.id}/rescan`);

		if (want('folder-list-view')) {
			await gotoReady(page, `/contents/${archive.id}`);
			await shotBetween(
				page,
				'folder-list-view',
				page.getByText(t('contents_form_list_view_label'), { exact: true }),
				// 自動の再スキャンの最後の行。英語では時刻の欄の後ろに言葉が無い。
				t('schedule_daily_after') === ''
					? page.getByLabel(t('schedule_time_label'))
					: page.getByText(t('schedule_daily_after'), { exact: true }),
				{ padTop: 16, padBottom: 16 }
			);
		}

		if (want('axis-add-dir')) {
			await gotoReady(page, `/contents/${archive.id}/axes`);
			await page.getByRole('button', { name: t('archive_axes_add_button') }).click();
			await waitForDialog(page);
			const d = dialog(page);
			await d.getByLabel(startsWith(t('archive_axes_dialog_name_label'))).fill(data().year);
			await page.locator('#axis-source').click();
			await page.getByRole('option', { name: t('archive_axes_source_option_dir_level') }).click();
			await d.getByText(t('archive_axes_dialog_dir_level_option', { level: 1 })).click();
			await d.getByLabel(startsWith(t('archive_axes_dialog_name_label'))).focus();
			await shotElement(page, 'axis-add-dir', d);
		}

		if (want('archive-axes-tab')) {
			await gotoReady(page, `/contents/${archive.id}/axes`);
			await shotBetween(
				page,
				'archive-axes-tab',
				page.getByRole('navigation', { name: t('contents_edit_tabs_label') }),
				page.getByRole('button', { name: t('archive_axes_add_button') }),
				{ padTop: 8, padBottom: 16 }
			);
		}

		if (want('axis-add-word')) {
			await gotoReady(page, `/contents/${archive.id}/axes`);
			await page.getByRole('button', { name: t('archive_axes_add_button') }).click();
			await waitForDialog(page);
			const d = dialog(page);
			await d.getByLabel(startsWith(t('archive_axes_dialog_name_label'))).fill(data().kind);
			await page.locator('#axis-source').click();
			await page
				.getByRole('option', { name: t('archive_axes_source_option_filename_word') })
				.click();
			await d.getByLabel(startsWith(t('archive_axes_dialog_name_label'))).focus();
			await shotElement(page, 'axis-add-word', d);
		}

		// 共有の中のコンテンツは、録音を足して5件。
		if (want('roots-delete')) {
			await gotoReady(page, '/admin/roots');
			await page.getByRole('button', { name: ROW_ACTIONS_BUTTON_NAME }).first().click();
			await page.getByRole('menuitem', { name: DELETE_MENU_ITEM_NAME }).click();
			await page.getByRole('alertdialog').waitFor();
			await shotElement(page, 'roots-delete', page.getByRole('alertdialog'));
		}

		const axes = [
			{ name: data().year, source: 'dirLevel', dirLevel: 1 },
			{ name: data().round, source: 'dirLevel', dirLevel: 2 },
			{ name: data().kind, source: 'filenameWord' }
		];
		for (const [position, axis] of axes.entries())
			await send(page.request, 'post', `/contents/${archive.id}/axes`, { ...axis, position });
		// マニュアルの手順 (06-archive-setup.md) と同じ表示タイトルにする。
		// 完全上書き型の PUT なので、作成時の応答をそのまま送り直す。
		await send(page.request, 'put', `/contents/${archive.id}`, {
			...archive,
			titleTemplate: `{${data().year}} {${data().round}} {fileName}`
		});

		if (want('axis-row-actions')) {
			await gotoReady(page, `/contents/${archive.id}/axes`);
			await shotBetween(
				page,
				'axis-row-actions',
				page.getByRole('heading', { name: t('archive_axes_section_title'), exact: true }),
				page.getByText(t('archive_axes_source_option_filename_word')),
				{
					padTop: 20,
					padBottom: 18
				}
			);
		}

		if (want('archive-axes-template')) {
			await gotoReady(page, `/contents/${archive.id}/axes`);
			await shotBetween(
				page,
				'archive-axes-template',
				page.getByRole('heading', { name: t('archive_axes_template_title') }),
				page.getByText(t('archive_axes_template_file_name_help'), { exact: false }),
				{ padTop: 16, padBottom: 16 }
			);
		}

		const tabs = page.getByRole('navigation', { name: t('contents_edit_tabs_label') });
		if (want('archive-items-tab')) {
			await gotoReady(page, `/contents/${archive.id}/items`);
			const thirdRow = page.getByRole('listitem').filter({ hasText: '.mp3' }).nth(2);
			await shotBetween(page, 'archive-items-tab', tabs, thirdRow, { padTop: 8, padBottom: 8 });
		}

		if (want('viewing-filter') || want('viewing-sort')) {
			const phone = await newPage(backend, PLAYER_WIDTH);
			await gotoReady(phone, `/archives/${archive.id}`);
			if (want('viewing-filter')) {
				await phone.getByRole('combobox', { name: startsWith(data().year) }).click();
				const options = phone.getByRole('listbox');
				await options.waitFor();
				await shotBetween(phone, 'viewing-filter', phone.getByRole('navigation').first(), options, {
					padTop: 8,
					padBottom: 16
				});
				await phone.keyboard.press('Escape');
			}
			if (want('viewing-sort')) {
				const sort = phone.getByRole('combobox', { name: startsWith(t('browse_sort_label')) });
				await sort.click();
				const options = phone.getByRole('listbox');
				await options.waitFor();
				await shotBetween(phone, 'viewing-sort', sort, options, { padTop: 16, padBottom: 16 });
			}
			await phone.context().close();
		}

		if (want('share-qr')) {
			// 使い捨ての backend は 127.0.0.1 で待ち受けるので、そのままだと QR コードでなく
			// 「他の端末からはつながりません」の案内になる。他の端末から開ける設置と同じ答えを返させる。
			await page.route('**/api/v1/connection-info', (route) =>
				route.fulfill({ json: { mdnsHostname: 'my-pc.local', port: 8370, lanAddresses: [] } })
			);
			await gotoReady(page, '/');
			await page.getByRole('button', { name: t('share_trigger_label') }).click();
			await page.getByRole('menuitem', { name: t('share_qr_menu_item') }).click();
			await shotElement(page, 'share-qr', dialog(page));
		}

		// ここからは、上の画像に写さないコンテンツを足して撮る。
		if (['edit-link', 'edit-visibility', 'viewing-home'].some(want)) {
			const created = (await send(page.request, 'post', '/contents', {
				type: 'link',
				url: 'https://www.jma.go.jp/bosai/forecast/',
				title: data().linkTitle,
				// 説明が空だと、サーバーがリンク先へ取りに行く。入れて足してから空に戻す (更新では取りに行かない)。
				description: data().linkTitle,
				visibility: 'public'
			})) as { id: number };
			// 完全上書き型の PUT なので、作成時の応答をそのまま送り直す。
			const link = (await send(page.request, 'put', `/contents/${created.id}`, {
				...created,
				description: null
			})) as { id: number };
			const notice = path.join(shared, data().documents, `${data().notice}.pdf`);
			writeFileSync(notice, '');
			await send(page.request, 'post', '/contents', {
				type: 'shortcut',
				title: data().notice,
				path: notice,
				visibility: 'public'
			});

			const basic = page.getByRole('heading', {
				name: t('contents_edit_basic_title'),
				exact: true
			});
			if (want('edit-link')) {
				await gotoReady(page, `/contents/${link.id}`);
				await shotSection(page, 'edit-link', basic);
			}
			if (want('edit-visibility')) {
				await gotoReady(page, `/contents/${link.id}`);
				await shotSection(
					page,
					'edit-visibility',
					basic,
					page
						.getByRole('form', { name: t('contents_edit_basic_title') })
						.getByRole('button', { name: t('action_save') }),
					{ top: page.getByText(t('contents_form_parent_label'), { exact: true }) }
				);
			}
			if (want('viewing-home')) {
				// 見る人の画面なので、ログインしていない人で撮る (編集のペンが出ない)。
				const phone = await newPage(backend, PLAYER_WIDTH, undefined, { anonymous: true });
				// リンクのサイトのアイコンと更新日は、サーバーがリンク先から取ってくる。撮るたびに変わらないよう、
				// 取れていないときの形 (ホスト名だけ) にする。
				await phone.route(
					(url) => url.pathname === '/api/v1/contents',
					async (route) => {
						const res = await route.fetch();
						const body = JSON.parse(await res.text(), (key, value) =>
							key === 'preview' ? null : value
						);
						await route.fulfill({ response: res, json: body });
					}
				);
				await gotoReady(phone, '/');
				await shotBetween(
					phone,
					'viewing-home',
					phone.getByRole('combobox', { name: startsWith(t('browse_sort_label')) }),
					browseLink(phone, data().homeLastRow),
					{ padTop: 16, padBottom: 16 }
				);
				await phone.context().close();
			}
		}

		// 閲覧の画面の「追加」から開く、リンクとグループの登録の窓。入れ終えて、追加を押す前の形にする。
		for (const [name, type] of [
			['add-link', 'link'],
			['add-group', 'group']
		] as const) {
			if (!want(name)) continue;
			// 入れた後は窓を Escape で閉じられないので、撮るたびにページを開き直す。
			const adder = await newPage(backend, DESKTOP_WIDTH);
			await gotoReady(adder, '/');
			await adder.getByRole('button', { name: BROWSE_ADD_BUTTON_NAME }).click();
			await adder.getByRole('menuitem', { name: BROWSE_ADD_MENU_ITEM_NAME[type] }).click();
			await waitForDialog(adder);
			if (type === 'link') await dialog(adder).getByLabel(/^URL/).fill('https://www.jma.go.jp/');
			else await dialog(adder).getByLabel(TITLE_LABEL).fill(data().groupExample);
			await shotElement(adder, name, dialog(adder));
			await adder.context().close();
		}

		// ログインしていない人のヘッダーとログイン画面。ヘッダーの「ログイン」の文字が出る幅で撮る。
		if (['header-login', 'header-language', 'header-theme', 'login'].some(want)) {
			const anon = await newPage(backend, DESKTOP_WIDTH, undefined, { anonymous: true });
			const banner = anon.getByRole('banner');
			const search = banner.getByRole('link', { name: t('search_nav_label') });
			const login = banner.getByRole('link', { name: t('login_nav_label') });
			if (want('header-login')) {
				await gotoReady(anon, '/');
				await shotAround(anon, 'header-login', [search, login]);
			}
			for (const [name, label] of [
				['header-language', 'language_toggle_label'],
				['header-theme', 'theme_toggle_label']
			] as const) {
				if (!want(name)) continue;
				await gotoReady(anon, '/');
				await banner.getByRole('button', { name: t(label) }).click();
				const menu = anon.getByRole('menu');
				await menu.waitFor();
				await shotAround(anon, name, [search, login, menu]);
				await anon.keyboard.press('Escape');
				await menu.waitFor({ state: 'hidden' });
			}
			if (want('login')) {
				await gotoReady(anon, '/login');
				await anon.getByLabel(LOGIN_USERNAME_LABEL).fill('admin');
				await anon.getByLabel(LOGIN_PASSWORD_LABEL).fill('password');
				await shotAround(anon, 'login', [
					anon.getByText(t('common_username_label'), { exact: true }),
					anon.getByRole('button', { name: t('login_submit') })
				]);
			}
			await anon.context().close();
		}

		if (want('viewing-viewer')) {
			// PDF のビューアー。前後に移るボタンが出るよう、同じフォルダーにもう1つ置く。
			const documents = path.join(shared, data().documents);
			writeFileSync(path.join(documents, data().noticePdf), await makePdf(data().noticeLines));
			const second = path.join(documents, `${data().notice}.pdf`);
			if (!existsSync(second)) writeFileSync(second, await makePdf([data().notice]));
			const phone = await newPage(backend, PLAYER_WIDTH, undefined, { height: PHONE_HEIGHT });
			await gotoReady(phone, `/folders/${folderIds.get(data().documents)}`);
			await phone.getByText(data().noticePdf, { exact: true }).click();
			// PDF.js が1ページ目を描き終えるのを待つ。
			await phone.waitForFunction(() =>
				[...document.querySelectorAll('canvas')].some(
					(canvas) => (canvas as HTMLCanvasElement).width > 0
				)
			);
			await phone.waitForTimeout(500);
			await phone.mouse.move(1, 1);
			await save(
				phone,
				'viewing-viewer',
				await phone.screenshot({
					animations: 'disabled',
					clip: { x: 0, y: 0, width: PLAYER_WIDTH, height: VIEWER_SHOT_HEIGHT }
				})
			);
			await phone.context().close();
		}

		if (want('archive-values')) {
			// 英語の語 (`main`・`notes`) で分ける例 (→ 07-archive-setup.md)。録音の画像を変えないよう、別のライブラリにする。
			const english = path.join(shared, data().english);
			mkdirSync(english);
			for (let unit = 1; unit <= 5; unit++)
				for (const word of ['main', 'notes'])
					writeFileSync(path.join(english, `unit${unit}_${word}.mp3`), '');
			const created = (await send(page.request, 'post', '/contents', {
				type: 'folder',
				title: data().english,
				path: english,
				visibility: 'public'
			})) as { id: number };
			// 完全上書き型の PUT なので、作成時の応答をそのまま送り直す。
			const library = (await send(page.request, 'put', `/contents/${created.id}`, {
				...created,
				listView: true,
				includePatterns: '*.mp3'
			})) as { id: number };
			await send(page.request, 'post', `/contents/${library.id}/rescan`);
			const axis = (await send(page.request, 'post', `/contents/${library.id}/axes`, {
				name: data().kind,
				source: 'filenameWord',
				position: 0
			})) as { id: number };
			await send(page.request, 'put', `/contents/${library.id}/axes/${axis.id}/values`, {
				values: [
					{ rawValue: 'main', displayName: data().main },
					{ rawValue: 'notes', displayName: data().notes }
				]
			});
			await gotoReady(page, `/contents/${library.id}/axes`);
			await page
				.getByRole('button', {
					name: `${data().kind} ${t('archive_axes_source_option_filename_word')}`
				})
				.click();
			const heading = page.getByRole('heading', {
				name: t('archive_axes_values_heading', { name: data().kind })
			});
			await heading.waitFor();
			// 行ごとの件数と、表の下の件数の集計は、表を出した後に問い合わせて出る。
			for (const word of ['main', 'notes'])
				await page
					.getByRole('button', {
						name: t('archive_axes_values_row_files_label', { value: word, count: 5 })
					})
					.waitFor();
			await page
				.getByText(startsWith(textBefore('archive_axes_preview_summary', 'matched')))
				.waitFor();
			await shotBetween(
				page,
				'archive-values',
				heading,
				page.getByRole('button', { name: t('archive_axes_values_add_row'), exact: true }),
				{ padTop: 20, padBottom: 16 }
			);
		}
	} finally {
		await closePagesAndStop(backend);
	}
}

/** セットアップの画面。管理者がまだいない backend でしか開けないので、別に起動する。 */
async function shootSetup() {
	if (!want('quickstart-setup')) return;
	let backend: Backend;
	starting = true;
	try {
		backend = await startBackend('weblav-manual-shots-setup-');
	} finally {
		starting = false;
	}
	liveBackends.add(backend);
	try {
		const phone = await newPage(backend, PLAYER_WIDTH, undefined, { anonymous: true });
		// トレイの「セットアップ」と同じ口で、開くためのトークンを受け取る (この PC の中からだけ通る)。
		const issued = await phone.request.post('/api/v1/setup/token');
		if (!issued.ok())
			throw new Error(`quickstart-setup: トークンを受け取れません (${issued.status()})`);
		await gotoReady(phone, withQuery('/setup', { token: await issued.text() }));
		// 入れ終えて、作成のボタンを押す前の形にする。
		await phone.getByLabel(USERNAME_LABEL).fill('admin');
		await phone.getByLabel(PASSWORD_LABEL).fill('password');
		await phone.getByLabel(PASSWORD_CONFIRM_LABEL).fill('password');
		await shotBetween(
			phone,
			'quickstart-setup',
			phone.locator('[data-slot="card"]').first(),
			phone.getByRole('button', { name: t('setup_submit') }),
			{ padBottom: 16 }
		);
	} finally {
		await closePagesAndStop(backend);
	}
}

/** 文の行を並べた PDF。ブラウザーの印刷で作るので、日本語の書体を埋め込める。 */
async function makePdf(lines: string[], { scale = 1 } = {}): Promise<Buffer> {
	const context = await browser.newContext();
	const page = await context.newPage();
	const [title, ...body] = lines;
	await page.setContent(
		`<html lang="${lang}"><body style="font-family: sans-serif; padding: 24px">` +
			`<h1 style="font-size: ${28 * scale}px">${title}</h1>` +
			body.map((line) => `<p style="font-size: ${14 * scale}px">${line}</p>`).join('') +
			'</body></html>'
	);
	const pdf = await page.pdf({ format: 'A4' });
	await context.close();
	return pdf;
}

/** はじめにの画像に写す写真。風景をキャンバスで描き、JPEG にして返す。順は SAMPLE の introPhotos と同じ。 */
async function makePhotos(): Promise<Buffer[]> {
	const context = await browser.newContext();
	const page = await context.newPage();
	const photos = await page.evaluate(() => {
		const draw: ((c: CanvasRenderingContext2D, w: number, h: number) => void)[] = [
			// 夕焼けの海
			(c, w, h) => {
				const sky = c.createLinearGradient(0, 0, 0, h / 2);
				sky.addColorStop(0, '#f6875c');
				sky.addColorStop(1, '#fbd38a');
				c.fillStyle = sky;
				c.fillRect(0, 0, w, h / 2);
				c.fillStyle = '#fbe7a6';
				c.beginPath();
				c.arc(w / 2, h / 2, h / 10, Math.PI, 0);
				c.fill();
				const sea = c.createLinearGradient(0, h / 2, 0, h);
				sea.addColorStop(0, '#6f7fa8');
				sea.addColorStop(1, '#40557f');
				c.fillStyle = sea;
				c.fillRect(0, h / 2, w, h / 2);
				c.fillStyle = '#fbe7a6';
				for (let i = 0; i < 14; i++) {
					const half = (h / 10) * (1 - i / 16);
					c.fillRect(w / 2 - half, h / 2 + 12 + i * 14, half * 2, 4);
				}
			},
			// 夜の街
			(c, w, h) => {
				c.fillStyle = '#141a33';
				c.fillRect(0, 0, w, h);
				for (let x = 0; x < w; x += 90) {
					const top = h * (0.35 + ((x * 7) % 30) / 100);
					c.fillStyle = '#232a44';
					c.fillRect(x, top, 80, h - top);
					c.fillStyle = '#f2d27a';
					for (let y = top + 16; y < h - 10; y += 28)
						for (let wx = x + 10; wx < x + 70; wx += 20)
							if ((wx * 3 + y * 5) % 7 < 3) c.fillRect(wx, y, 8, 12);
				}
			},
			// 山の朝
			(c, w, h) => {
				const sky = c.createLinearGradient(0, 0, 0, h);
				sky.addColorStop(0, '#bfe0f2');
				sky.addColorStop(1, '#eef6f2');
				c.fillStyle = sky;
				c.fillRect(0, 0, w, h);
				c.fillStyle = '#7f95a3';
				c.beginPath();
				c.moveTo(0, h * 0.8);
				c.lineTo(w * 0.45, h * 0.3);
				c.lineTo(w, h * 0.75);
				c.lineTo(w, h);
				c.lineTo(0, h);
				c.fill();
				c.fillStyle = '#6a9a6b';
				c.fillRect(0, h * 0.82, w, h * 0.18);
			},
			// 湖と森
			(c, w, h) => {
				c.fillStyle = '#cfe7f1';
				c.fillRect(0, 0, w, h * 0.45);
				c.fillStyle = '#2f6a45';
				for (let x = 0; x < w; x += 40) {
					c.beginPath();
					c.moveTo(x, h * 0.6);
					c.lineTo(x + 20, h * 0.3);
					c.lineTo(x + 40, h * 0.6);
					c.fill();
				}
				c.fillRect(0, h * 0.55, w, h * 0.1);
				c.fillStyle = '#4e86a8';
				c.fillRect(0, h * 0.65, w, h * 0.35);
			},
			// 花畑
			(c, w, h) => {
				c.fillStyle = '#cde9f7';
				c.fillRect(0, 0, w, h * 0.4);
				c.fillStyle = '#6cae5a';
				c.fillRect(0, h * 0.4, w, h * 0.6);
				const colors = ['#f2b8c6', '#f6e27a', '#ffffff', '#e98a8a'];
				for (let i = 0; i < 400; i++) {
					c.fillStyle = colors[i % colors.length];
					const x = (i * 97) % w;
					const y = h * 0.45 + ((i * 53) % (h * 0.55));
					c.beginPath();
					c.arc(x, y, 6, 0, Math.PI * 2);
					c.fill();
				}
			},
			// 雪の日
			(c, w, h) => {
				c.fillStyle = '#d9dee3';
				c.fillRect(0, 0, w, h);
				c.fillStyle = '#ffffff';
				c.fillRect(0, h * 0.65, w, h * 0.35);
				c.fillStyle = '#7b8792';
				for (let x = 60; x < w; x += 180) {
					c.beginPath();
					c.moveTo(x, h * 0.68);
					c.lineTo(x + 40, h * 0.35);
					c.lineTo(x + 80, h * 0.68);
					c.fill();
				}
			}
		];
		return draw.map((paint) => {
			const canvas = document.createElement('canvas');
			canvas.width = 1200;
			canvas.height = 800;
			paint(canvas.getContext('2d')!, canvas.width, canvas.height);
			return canvas.toDataURL('image/jpeg', 0.85).split(',')[1];
		});
	});
	await context.close();
	return photos.map((photo) => Buffer.from(photo, 'base64'));
}

/** 音の鳴る WAV。プレイヤーの音の形の表示が動くよう、いくつかの高さの音を重ねる。 */
function toneWav(seconds: number): Buffer {
	const wav = silentWav(seconds);
	for (let i = 44; i < wav.length; i++) {
		const t = (i - 44) / 8000;
		const v =
			Math.sin(2 * Math.PI * 220 * t) +
			0.6 * Math.sin(2 * Math.PI * 440 * t) +
			0.3 * Math.sin(2 * Math.PI * 660 * t);
		wav[i] = 128 + Math.round(v * 40);
	}
	return wav;
}

/** `dir` の中の全部の更新日を `mtime` にする。一覧に出る日付を、撮るたびに変えないため。 */
function setMtimes(dir: string, mtime: Date) {
	for (const entry of readdirSync(dir, { withFileTypes: true })) {
		const entryPath = path.join(dir, entry.name);
		if (entry.isDirectory()) setMtimes(entryPath, mtime);
		utimesSync(entryPath, mtime, mtime);
	}
}

/**
 * はじめにの画像。ホーム・写真のフォルダー・写真を開いたところの3つを、音声を鳴らしたまま撮り、
 * スマートフォンの枠に入れて並べる (intro-overview.html)。ほかの画像と混ざらないよう、別のデータで撮る。
 */
async function shootIntro() {
	if (!want('intro-overview')) return;
	const d = data();
	const base = mkdtempSync(path.join(tmpdir(), 'weblav-manual-shots-intro-'));
	tempDirs.push(base);
	const shared = path.join(base, d.school);
	const photosDir = path.join(shared, d.photos);
	mkdirSync(photosDir, { recursive: true });
	for (const [i, photo] of (await makePhotos()).entries())
		writeFileSync(path.join(photosDir, `${d.introPhotos[i]}.jpg`), photo);
	mkdirSync(path.join(shared, d.introMaterials));
	const notice = path.join(shared, d.introMaterials, `${d.notice}.pdf`);
	// 一覧の行に縮小画像が出るので、小さくしても文書と分かるよう大きな字で書く。
	writeFileSync(notice, await makePdf(d.noticeLines, { scale: 4 }));
	mkdirSync(path.join(shared, d.music));
	writeFileSync(path.join(shared, d.music, d.introTrack), toneWav(INTRO_TRACK_SECONDS));
	setMtimes(shared, INTRO_MTIME);
	const sharedPath = realpathSync(shared);

	const backend = await startWithAdmin();
	try {
		const admin = await newPage(backend, DESKTOP_WIDTH);
		await send(admin.request, 'post', '/admin/roots', { path: sharedPath });
		const ids = new Map<string, number>();
		for (const title of [d.photos, d.introMaterials, d.music]) {
			const folder = (await send(admin.request, 'post', '/contents', {
				type: 'folder',
				path: path.join(sharedPath, title),
				visibility: 'public'
			})) as { id: number };
			ids.set(title, folder.id);
		}
		await send(admin.request, 'post', '/contents', {
			type: 'group',
			title: d.introGroup,
			visibility: 'public'
		});
		await send(admin.request, 'post', '/contents', {
			type: 'shortcut',
			title: d.notice,
			path: path.join(sharedPath, d.introMaterials, `${d.notice}.pdf`),
			visibility: 'public'
		});
		await admin.context().close();

		// 見る人の画面なので、ログインしていない人で撮る。
		const phone = await newPage(backend, PLAYER_WIDTH, undefined, {
			anonymous: true,
			height: PHONE_HEIGHT
		});
		const screens: { png: string; url: string }[] = [];
		const capture = async () => {
			await phone.mouse.move(1, 1);
			// 音の形の表示は鳴っている間だけ動くので、止めずに少し待ってから撮る。
			await phone.waitForTimeout(600);
			const url = new URL(phone.url());
			screens.push({
				png: (await phone.screenshot()).toString('base64'),
				url: `my-pc.local:8370${url.pathname === '/' ? '' : url.pathname}`
			});
		};
		await gotoReady(phone, `/folders/${ids.get(d.music)}`);
		await phone.getByRole('button', { name: d.introTrack, exact: true }).click();
		await phone.waitForFunction(() => (document.querySelector('audio')?.currentTime ?? 0) > 0);
		await phone.evaluate(() => {
			document.querySelector('audio')!.currentTime = 7;
		});
		// ページを読み込み直すと音が止まるので、画面の中のリンクで移る。
		await phone.getByRole('link', { name: 'WebLAV' }).click();
		await browseLink(phone, d.photos).waitFor();
		// 今月の予定の行には、PDF の縮小画像が出る。
		await waitForThumbnails(phone, { timeout: 10_000 });
		await capture();
		await browseLink(phone, d.photos).click();
		await phone.waitForURL(/\/folders\/\d+/);
		await waitForThumbnails(phone, { timeout: 10_000 });
		await capture();
		// 一覧の先頭の写真を開く。タイトル順は文字の符号の順なので、同じ順に並べて先頭を選ぶ。
		const first = [...d.introPhotos].sort()[0];
		await phone.getByText(`${first}.jpg`, { exact: true }).click();
		await phone.waitForFunction(() =>
			[...document.querySelectorAll<HTMLImageElement>('[role=dialog] img')].some(
				(img) => img.complete && img.naturalWidth > 0
			)
		);
		await capture();
		await phone.context().close();

		const context = await browser.newContext({
			viewport: { width: 760, height: 480 },
			deviceScaleFactor: 2
		});
		const page = await context.newPage();
		await page.goto(pathToFileURL(INTRO_HTML).href);
		await page.evaluate(async (shots) => {
			const imgs = [...document.querySelectorAll<HTMLImageElement>('[data-screen]')];
			const urls = [...document.querySelectorAll<HTMLElement>('[data-url]')];
			shots.forEach((shot, i) => {
				imgs[i].src = `data:image/png;base64,${shot.png}`;
				urls[i].textContent = shot.url;
			});
			await Promise.all(imgs.map((img) => img.decode()));
			await document.fonts.ready;
		}, screens);
		await save(page, 'intro-overview', await page.locator('#fig').screenshot());
		await context.close();
	} finally {
		await closePagesAndStop(backend);
	}
}

/** 仕組みの図。画面ではないので HTML で描いたものを撮る。 */
async function shootDiagram() {
	if (!want('how-it-works')) return;
	const context = await browser.newContext({
		viewport: { width: 700, height: 340 },
		deviceScaleFactor: 2
	});
	const page = await context.newPage();
	// 図の文言は HTML の中に言語ごとに持つ (data-en)。
	await page.goto(withQuery(pathToFileURL(DIAGRAM_HTML).href, { lang }));
	// 英語の図は、書体を読み終えてから線の位置を直す (how-it-works.html)。それも待つ。
	await page.evaluate(async () => {
		await ((window as { layoutReady?: Promise<void> }).layoutReady ?? document.fonts.ready);
	});
	await save(page, 'how-it-works', await page.locator('#fig').screenshot());
	await context.close();
}

async function cleanUp() {
	// 一時フォルダーは await より前に消す。backend の起動中の中断では、backend-process.ts が先に終えることがある。
	for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
	await browser?.close().catch(() => {});
	await Promise.all([...liveBackends].map(stopBackend));
}

// Ctrl-C で抜けても、backend・一時フォルダーを残さない (→ backend-process.ts の onInterrupt と同じ)。
// 撮影の途中の操作はブラウザーを閉じたところで失敗するので、その失敗は出さずに 130 で終える。
let interrupted = false;
process.once('SIGINT', () => {
	interrupted = true;
	// 起動の最中なら、起動しかけの backend を止めてから終えるのは backend-process.ts の側。
	// こちらが先に終えると、あちらの片付けを途中で切ってしまう。
	const exitAfter = !starting;
	void cleanUp().finally(() => {
		if (exitAfter) process.exit(130);
	});
});

try {
	for (const l of langs) {
		lang = l;
		await launchBrowser();
		if (ADMIN_SHOTS.some(want)) await shootAdmin();
		await shootSetup();
		await shootDiagram();
		await shootIntro();
		await browser.close();
	}
} catch (err) {
	if (!interrupted) throw err;
} finally {
	await cleanUp();
}
// backend の起動の早い段階で中断されると、どちらの SIGINT の処理も終えないままここに来る。
if (interrupted) process.exitCode = 130;
