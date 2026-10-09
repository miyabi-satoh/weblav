// `just manual-shots` から呼ばれる。マニュアルの画像 (docs/manual/ja/images) のうち、
// 管理画面と仕組みの図を、使い捨ての backend で撮り直して WebP で書き出す (→ docs/help.md)。
// 画面の文言や見た目を変えたら流し直す。名前を渡すと、その画像だけを撮る (`just manual-shots roots-page`)。
//
// ここに無い画像 (タスクトレイ・OS の窓・閲覧側の画面など) は手で撮っている。
// 撮れるものを足すときは、下の撮影の流れに足す。データは元の画像に写っていた名前に合わせてある。

import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { chromium, type APIRequestContext, type Locator, type Page, type Route } from 'playwright';
import {
	CHOOSE_PATH_BUTTON_NAME,
	CONTENT_ADD_BUTTON_NAME,
	DELETE_MENU_ITEM_NAME,
	DIR_PICKER_NAME,
	ROOT_ADD_BUTTON_NAME,
	ROW_ACTIONS_BUTTON_NAME,
	USE_THIS_FOLDER_BUTTON_NAME,
	chooseContentType,
	openAddDialog,
	waitForDialog
} from '../e2e/helpers.ts';
import { insertTestAdmin, startBackend, type Backend } from './backend-process.ts';
import { TEST_ADMIN } from './test-account.ts';

const REPO_ROOT = path.resolve(import.meta.dirname, '../..');
const OUT_DIR = path.join(REPO_ROOT, 'docs/manual/ja/images');
const DIAGRAM_HTML = path.join(import.meta.dirname, 'manual-shots/how-it-works.html');

/** 撮れる画像。Pro の WebLAV・Free の WebLAV・図のどれで撮るかで分ける。 */
const ADMIN_SHOTS = [
	'roots-page',
	'settings-page',
	'maintenance-backup',
	'contents-page',
	'row-menu',
	'register-group-menu',
	'quickstart-add-folder',
	'add-folder-form',
	'add-folder-picker',
	'add-folder-ready',
	'quickstart-roots',
	'add-archive-picker',
	'axis-add-dir',
	'roots-delete',
	'axis-row-actions'
];
const FREE_SHOTS = ['settings-pro', 'pro-link'];
const DIAGRAM_SHOTS = ['how-it-works'];

/** 撮る画像の名前。既定は全部。 */
const ONLY = new Set(process.argv.slice(2));
const unknown = [...ONLY].filter(
	(name) => ![...ADMIN_SHOTS, ...FREE_SHOTS, ...DIAGRAM_SHOTS].includes(name)
);
if (unknown.length > 0) {
	// 名前を打ち間違えたまま、何も撮らずに成功で終えないため。
	console.error(
		`撮れない画像の名前です: ${unknown.join(', ')}\n撮れるもの: ${[...ADMIN_SHOTS, ...FREE_SHOTS, ...DIAGRAM_SHOTS].join(', ')}`
	);
	process.exit(1);
}
const want = (name: string) => ONLY.size === 0 || ONLY.has(name);

/** 管理画面を撮る幅。表の列が詰まらず、画像が本文の幅に収まる。 */
const DESKTOP_WIDTH = 800;
/** スマートフォン幅の画像 (タブがアイコンになる) を撮る幅。 */
const MOBILE_WIDTH = 450;
/** 欄が画面の下にはみ出さない高さ。はみ出すと clip の外が切れる。 */
const VIEWPORT_HEIGHT = 2400;
/** WebP の品質。1枚あたり数十 KB に収まり、文字が滲まない (→ docs/help.md)。 */
const WEBP_QUALITY = 0.85;
/**
 * 画像に写すパス。OS で違う画面は Windows のものを載せる (→ docs/help.md) ので、
 * 手元のパスを API の応答の中でこれに置き換える。
 */
const WINDOWS_SHARED_PATH = 'C:\\Users\\user\\Documents\\共有';

// Ctrl-C はこちらで受けて片付ける (下の SIGINT)。Playwright に先にブラウザーを閉じさせない。
const browser = await chromium.launch({ handleSIGINT: false });
/** makeSharedDir が作った一時フォルダー。最後に消す。 */
const tempDirs: string[] = [];

/** 公開できるフォルダー「共有」と、その中のフォルダー・ファイルを作り、絶対パスを返す。 */
function makeSharedDir(): string {
	const base = mkdtempSync(path.join(tmpdir(), 'weblav-manual-shots-files-'));
	tempDirs.push(base);
	const shared = path.join(base, '共有');
	const files = [
		'写真/運動会.jpg',
		'音楽/校歌.mp3',
		'書類/お知らせ.pdf',
		'動画/海.mp4',
		'英検/準2級.pdf',
		'下書き/メモ.txt'
	];
	for (const year of ['2024', '2025'])
		for (const round of ['第1回', '第2回', '第3回'])
			for (const kind of ['問題', '解答']) files.push(`録音/${year}/${round}/${kind}.mp3`);
	for (const rel of files) {
		const file = path.join(shared, rel);
		mkdirSync(path.dirname(file), { recursive: true });
		writeFileSync(file, '');
	}
	mkdirSync(path.join(shared, '教材'));
	// サーバーは実パスで返す (macOS の一時フォルダーは /private/var/...)。置き換えはこちらに合わせる。
	return realpathSync(shared);
}

async function send(request: APIRequestContext, method: string, apiPath: string, data?: unknown) {
	const res = await request.fetch(`/api/v1${apiPath}`, { method, data });
	if (!res.ok())
		throw new Error(`${method} ${apiPath} が失敗しました (${res.status()}): ${await res.text()}`);
	return res.status() === 204 ? null : ((await res.json()) as { id: number });
}

async function upload(page: Page, title: string, fileName: string, mimeType: string) {
	const res = await page.request.post('/api/v1/contents/upload', {
		multipart: { title, file: { name: fileName, mimeType, buffer: Buffer.from('') } }
	});
	if (!res.ok()) throw new Error(`${title} のアップロードが失敗しました (${res.status()})`);
}

/** 管理者でログインしたページを開く。ログイン画面は撮らないので API で入る。 */
async function newPage(backend: Backend, width: number, sharedPath?: string): Promise<Page> {
	const context = await browser.newContext({
		viewport: { width, height: VIEWPORT_HEIGHT },
		deviceScaleFactor: 2, // 画面は2倍で撮る (→ docs/help.md)
		baseURL: backend.baseURL,
		locale: 'ja-JP'
	});
	await context.addCookies([{ name: 'WEBLAV_LOCALE', value: 'ja', url: backend.baseURL }]);
	const page = await context.newPage();
	await send(page.request, 'POST', '/auth/login', {
		username: TEST_ADMIN.username,
		password: TEST_ADMIN.password
	});
	if (sharedPath) {
		const local = JSON.stringify(sharedPath).slice(1, -1);
		const windows = JSON.stringify(WINDOWS_SHARED_PATH).slice(1, -1);
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

async function startWithAdmin(pro: boolean): Promise<Backend> {
	starting = true;
	let backend: Backend;
	try {
		backend = await startBackend('weblav-manual-shots-', { pro });
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

async function stopBackend(backend: Backend) {
	liveBackends.delete(backend);
	await backend.stop();
}

async function goto(page: Page, url: string) {
	await page.goto(url);
	await page.waitForLoadState('networkidle');
	await page.evaluate(() => document.fonts.ready);
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
	writeFileSync(path.join(OUT_DIR, `${name}.webp`), Buffer.from(webp, 'base64'));
	console.log(`[shot] ${name}`);
}

/** 要素の位置。画面の文言を変えて要素が見つからなくなったら、どの画像のどの要素かを出して止める。 */
async function box(name: string, locator: Locator) {
	const found = await locator.boundingBox();
	if (!found) throw new Error(`${name}: 要素が見つかりません (${locator})`);
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
 */
async function shotSection(page: Page, name: string, heading: Locator, bottom?: Locator) {
	const PAD = 24;
	await heading.evaluate((el) => el.scrollIntoView({ block: 'start' }));
	await page.evaluate(() => window.scrollBy(0, -40));
	const section = page.locator('section').filter({ has: heading }).last();
	const s = await box(name, section);
	const h = await box(name, heading);
	const b = bottom ? await box(name, bottom) : undefined;
	const end = b ? b.y + b.height + 16 : s.y + s.height;
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

/** ダイアログなどの要素だけを撮る。ボタンにホバーの色が付かないよう、ポインターを外してから。 */
async function shotElement(page: Page, name: string, locator: Locator) {
	await page.mouse.move(1, 1);
	await save(page, name, await locator.screenshot({ animations: 'disabled' }));
}

const adminNav = (page: Page) =>
	page.getByRole('navigation').filter({ hasText: 'サイト設定' }).last();
const row = (page: Page, title: string) => page.getByRole('row').filter({ hasText: title }).first();
const dialog = (page: Page) => page.getByRole('dialog').last();
/** 登録の画面の上に重なって開く、フォルダーを選ぶ画面。 */
const picker = (page: Page) => page.getByRole('dialog', { name: DIR_PICKER_NAME });

/** Pro の WebLAV で撮るもの。コンテンツを足しながら、元の画像と同じ件数の時点で撮る。 */
async function shootAdmin() {
	const backend = await startWithAdmin(true);
	try {
		const shared = makeSharedDir();
		const page = await newPage(backend, DESKTOP_WIDTH, shared);
		await send(page.request, 'POST', '/admin/roots', { path: shared });

		if (want('roots-page')) {
			await goto(page, '/admin/roots');
			await shotBetween(
				page,
				'roots-page',
				adminNav(page),
				page.getByRole('button', { name: ROOT_ADD_BUTTON_NAME }),
				{ padBottom: 16 }
			);
		}

		for (const title of ['写真', '音楽', '書類'])
			await send(page.request, 'POST', '/contents', {
				type: 'folder',
				path: path.join(shared, title),
				visibility: 'public'
			});
		const group = await send(page.request, 'POST', '/contents', {
			type: 'group',
			title: '旅行の思い出',
			visibility: 'public'
		});
		await send(page.request, 'POST', '/contents', {
			type: 'folder',
			path: path.join(shared, '動画'),
			parentId: group!.id,
			visibility: 'public'
		});
		await upload(page, '今月の予定', '今月の予定.pdf', 'application/pdf');
		await upload(page, '集合写真', '集合写真.jpg', 'image/jpeg');

		if (want('settings-page') || want('maintenance-backup')) {
			await goto(page, '/admin/settings');
			if (want('settings-page'))
				await shotBetween(
					page,
					'settings-page',
					adminNav(page),
					page.getByRole('heading', { name: 'サイト設定', level: 1 }),
					{ padBottom: 24 }
				);
			if (want('maintenance-backup'))
				await shotSection(
					page,
					'maintenance-backup',
					page.getByRole('heading', { name: 'バックアップ' })
				);
		}

		await goto(page, '/admin/contents');
		if (want('contents-page'))
			await shotBetween(page, 'contents-page', adminNav(page), row(page, '書類'));
		if (want('row-menu')) {
			await row(page, '写真').getByRole('button', { name: ROW_ACTIONS_BUTTON_NAME }).click();
			await page.getByRole('menu').waitFor();
			await shotBetween(page, 'row-menu', row(page, '写真'), row(page, '書類'), { padTop: 8 });
			await page.keyboard.press('Escape');
		}
		if (want('register-group-menu')) {
			await row(page, '旅行の思い出')
				.getByRole('button', { name: ROW_ACTIONS_BUTTON_NAME })
				.click();
			await page.getByRole('menu').waitFor();
			await shotBetween(page, 'register-group-menu', row(page, '書類'), row(page, '集合写真'));
			await page.keyboard.press('Escape');
		}
		const folderShots = [
			'quickstart-add-folder',
			'add-folder-form',
			'add-folder-picker',
			'add-folder-ready'
		];
		if (folderShots.some(want)) {
			await openAddDialog(page, CONTENT_ADD_BUTTON_NAME);
			if (want('quickstart-add-folder'))
				await shotElement(page, 'quickstart-add-folder', dialog(page));
			await chooseContentType(page, 'folder');
			if (want('add-folder-form')) await shotElement(page, 'add-folder-form', dialog(page));
			await dialog(page).getByRole('button', { name: CHOOSE_PATH_BUTTON_NAME }).click();
			await picker(page).waitFor();
			await dialog(page).getByRole('button', { name: '共有' }).click();
			await dialog(page).getByRole('button', { name: '教材' }).click();
			await dialog(page).getByText('この中にフォルダーはありません。').waitFor();
			if (want('add-folder-picker')) await shotElement(page, 'add-folder-picker', dialog(page));
			await dialog(page).getByRole('button', { name: USE_THIS_FOLDER_BUTTON_NAME }).click();
			await dialog(page).getByRole('button', { name: '選び直す...' }).waitFor();
			if (want('add-folder-ready')) await shotElement(page, 'add-folder-ready', dialog(page));
		}

		// 共有の中のコンテンツは、写真・音楽・書類・動画の4件。
		if (want('quickstart-roots')) {
			const narrow = await newPage(backend, MOBILE_WIDTH, shared);
			await goto(narrow, '/admin/roots');
			const rootRow = narrow.getByText(WINDOWS_SHARED_PATH).locator('xpath=ancestor::li[1]');
			await shotBetween(narrow, 'quickstart-roots', adminNav(narrow), rootRow);
			await narrow.context().close();
		}

		if (want('add-archive-picker')) {
			await goto(page, '/admin/contents');
			await openAddDialog(page, CONTENT_ADD_BUTTON_NAME);
			await chooseContentType(page, 'archive');
			await dialog(page).getByRole('button', { name: CHOOSE_PATH_BUTTON_NAME }).click();
			await picker(page).waitFor();
			await dialog(page).getByRole('button', { name: '共有' }).click();
			await dialog(page).getByRole('button', { name: '録音' }).click();
			await dialog(page).getByRole('button', { name: '2025' }).waitFor();
			await shotElement(page, 'add-archive-picker', dialog(page));
		}

		const archive = await send(page.request, 'POST', '/contents', {
			type: 'archive',
			title: '録音',
			path: path.join(shared, '録音'),
			visibility: 'public',
			extensions: 'mp3'
		});
		// 階層ごとの値は、索引を作ってから出る。
		await send(page.request, 'POST', `/contents/${archive!.id}/rescan`);

		if (want('axis-add-dir')) {
			await goto(page, `/admin/contents/${archive!.id}/axes`);
			await page.getByRole('button', { name: '軸を追加...' }).click();
			await waitForDialog(page);
			const d = dialog(page);
			await d.getByLabel(/^軸名/).fill('年度');
			await page.locator('#axis-source').click();
			await page.getByRole('option', { name: 'フォルダーの階層' }).click();
			await d.getByText('第1階層').click();
			await d.getByLabel(/^軸名/).focus();
			await shotElement(page, 'axis-add-dir', d);
		}

		// 共有の中のコンテンツは、録音を足して5件。
		if (want('roots-delete')) {
			await goto(page, '/admin/roots');
			await page.getByRole('button', { name: ROW_ACTIONS_BUTTON_NAME }).first().click();
			await page.getByRole('menuitem', { name: DELETE_MENU_ITEM_NAME }).click();
			await shotElement(page, 'roots-delete', page.getByRole('alertdialog'));
		}

		if (want('axis-row-actions')) {
			const axes = [
				{ name: '年度', source: 'dirLevel', dirLevel: 1 },
				{ name: '回', source: 'dirLevel', dirLevel: 2 },
				{ name: '種類', source: 'filenameWord' }
			];
			for (const [position, axis] of axes.entries())
				await send(page.request, 'POST', `/contents/${archive!.id}/axes`, { ...axis, position });
			await goto(page, `/admin/contents/${archive!.id}/axes`);
			await shotBetween(
				page,
				'axis-row-actions',
				page.getByRole('heading', { name: '軸', exact: true }),
				page.getByText('ファイル名に含まれる語'),
				{
					padTop: 20,
					padBottom: 18
				}
			);
		}
	} finally {
		// 差し替えた応答が、止めた backend へ問い合わせないよう、先にページを閉じる。
		await Promise.all(browser.contexts().map((context) => context.close()));
		await stopBackend(backend);
	}
}

/** Free の WebLAV で撮るもの (Pro の欄)。 */
async function shootFree() {
	const backend = await startWithAdmin(false);
	try {
		const shared = makeSharedDir();
		const page = await newPage(backend, DESKTOP_WIDTH);
		await send(page.request, 'POST', '/admin/roots', { path: shared });
		await send(page.request, 'POST', '/contents', {
			type: 'archive',
			title: '録音',
			path: path.join(shared, '録音'),
			visibility: 'public',
			extensions: 'mp3'
		});
		for (const [title, url] of [
			['学校のお知らせ', 'https://example.com/news'],
			['時間割', 'https://example.com/timetable'],
			['給食の献立', 'https://example.com/lunch']
		])
			await send(page.request, 'POST', '/contents', {
				type: 'link',
				title,
				url,
				visibility: 'public'
			});

		const heading = page.getByRole('heading', { name: 'Pro', exact: true });
		if (want('settings-pro')) {
			await goto(page, '/admin/settings');
			await shotSection(page, 'settings-pro', heading);
		}
		if (want('pro-link')) {
			// 窓口へは行かず、登録しかけの状態を返す。QR コードに入る URL は本物と同じ形 (src/api/pro.rs の link_url)。
			const linking = async (route: Route) => {
				const body = await (await page.request.get('/api/v1/admin/pro')).json();
				body.linkUrl = 'https://weblav.amiiby.com/account/link?r=sample&name=WebLAV&pc=PC';
				await route.fulfill({ json: body });
			};
			await page.route('**/api/v1/admin/pro/link', linking);
			await page.route('**/api/v1/admin/pro/link/poll', linking);
			await goto(page, '/admin/settings');
			await page.getByRole('button', { name: 'Pro にする' }).click();
			const cancel = page.getByRole('button', { name: 'やめる' });
			await cancel.waitFor();
			await shotSection(page, 'pro-link', heading, cancel);
		}
	} finally {
		// 差し替えた応答が、止めた backend へ問い合わせないよう、先にページを閉じる。
		await Promise.all(browser.contexts().map((context) => context.close()));
		await stopBackend(backend);
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
	await page.goto(pathToFileURL(DIAGRAM_HTML).href);
	await page.evaluate(() => document.fonts.ready);
	await save(page, 'how-it-works', await page.locator('#fig').screenshot());
	await context.close();
}

async function cleanUp() {
	// 一時フォルダーは await より前に消す。backend の起動中の中断では、backend-process.ts が先に終えることがある。
	for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
	await browser.close().catch(() => {});
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
	if (ADMIN_SHOTS.some(want)) await shootAdmin();
	if (FREE_SHOTS.some(want)) await shootFree();
	await shootDiagram();
} catch (err) {
	if (!interrupted) throw err;
} finally {
	await cleanUp();
}
// backend の起動の早い段階で中断されると、どちらの SIGINT の処理も終えないままここに来る。
if (interrupted) process.exitCode = 130;
