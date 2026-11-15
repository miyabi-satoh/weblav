// macOS の自動の確認 (`just macos-check`) の土台。OS とつながる所 (メニューバー・フォルダー選択の窓・ログイン項目・
// ごみ箱・mDNS の名前) を、配る形の WebLAV.app で動かして読む (→ e2e/README.md「macOS の確認」)。
//
// メニューバーと窓はアクセシビリティの API (AXUIElement) を `osascript` の JXA から直に呼んで扱う。System Events からは
// WebLAV の窓が見えない (アイコンを押しても届かない)。キーは System Events で送る。回す端末にアクセシビリティの許可が要る。
//
// 確かめ用の WebLAV は、常用のものと別のデータの置き場 (`WEBLAV_HOME`) とポートで並べて動かす。常用のものは止めない。
import { execFile, spawn } from 'node:child_process';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);

/** 常用の WebLAV の束。ほかの束で回すときは環境変数 `WEBLAV_APP` で指す */
export const REGULAR_APP = '/Applications/WebLAV.app';
export const APP = process.env.WEBLAV_APP ?? REGULAR_APP;
const EXECUTABLE = path.join(APP, 'Contents', 'MacOS', 'weblav');
// サンドボックスで動くので、書ける場所はコンテナの中に限られる。コンテナは署名の識別子で決まり、常用のものと同じ
const CONTAINER = path.join(os.homedir(), 'Library', 'Containers', 'com.amiiby.weblav', 'Data');
export const HOME = path.join(CONTAINER, 'weblav-e2e');
// 常用のもの (既定 8370) とぶつからない番号
export const PORT = 18370;
export const PASSWORD = 'e2e-password-1';

/** 共有にするフォルダーを、毎回新しく作る。窓で選んだ場所はサンドボックスの外でも読み書きできる */
export async function workDir(name) {
	return fs.mkdtemp(path.join(await fs.realpath(os.tmpdir()), `weblav-e2e-${name}-`));
}

export async function waitFor(read, ok, { label, timeout = 10_000, interval = 200 } = {}) {
	const deadline = Date.now() + timeout;
	let last;
	for (;;) {
		last = await read();
		if (ok(last)) return last;
		if (Date.now() > deadline)
			throw new Error(
				`${label ?? '待ち'}: ${timeout}ms たっても満たされない (最後の値: ${JSON.stringify(last)})`
			);
		await new Promise((resolve) => setTimeout(resolve, interval));
	}
}

export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** 後片付けの手順を順に流す。1つが落ちても残りを片付け、落ちたものは出す */
export async function cleanUp(steps) {
	for (const step of steps) {
		try {
			await step();
		} catch (error) {
			console.error(error);
		}
	}
}

/** ファイルやフォルダーが無いことを確かめる */
export async function assertMissing(file, message) {
	const exists = await fs.access(file).then(
		() => true,
		() => false
	);
	if (exists) throw new Error(message);
}

/** 共有の中の一覧 (`Api.browse`) の名前を、並べて返す */
export const entryNames = (browse) => browse.entries.map((entry) => entry.name).sort();

/** JXA を流して、返した値 (JSON にできるもの) を受け取る。`args` は台本の中で `args` として読める */
export async function jxa(body, args = {}) {
	const script = `function run(argv) { const args = JSON.parse(argv[0]); const result = (() => { ${body} })(); return JSON.stringify(result === undefined ? null : result); }`;
	const { stdout } = await execFileAsync(
		'osascript',
		['-l', 'JavaScript', '-e', script, JSON.stringify(args)],
		{
			maxBuffer: 16 * 1024 * 1024
		}
	);
	return JSON.parse(stdout.trim() || 'null');
}

// アクセシビリティの API の読み書き。値の型ごとに読み分ける
const AX_READ = `
	ObjC.import('AppKit');
	ObjC.import('ApplicationServices');
	const axGet = (el, name) => { const ref = Ref(); return el && $.AXUIElementCopyAttributeValue(el, $(name), ref) === 0 ? ObjC.castRefToObject(ref[0]) : null; };
	const axStr = (el, name) => { const v = axGet(el, name); return v && v.isKindOfClass($.NSString) ? v.js : null; };
	const axBool = (el, name) => { const v = axGet(el, name); return v && v.isKindOfClass($.NSNumber) ? v.boolValue : null; };
	const axKids = (el) => { const v = axGet(el, 'AXChildren'); const out = []; for (let i = 0; v && i < v.count; i++) out.push(v.objectAtIndex(i)); return out; };
`;

// 確かめ用の WebLAV (args.pid) を読み書きする
const AX = `${AX_READ}
	const app = $.AXUIElementCreateApplication(args.pid);
	const icon = () => axKids(axGet(app, 'AXExtrasMenuBar'))[0] ?? null;
	// メニューの要素は、アイコンを一度押すまでできない。押して出たメニューは、すぐ取り消して閉じる
	const menuItems = () => {
		if (axKids(icon()).length === 0) {
			$.AXUIElementPerformAction(icon(), $('AXPress'));
			for (let i = 0; i < 20 && axKids(icon()).length === 0; i++) delay(0.1);
			const menu = axKids(icon())[0];
			if (menu) $.AXUIElementPerformAction(menu, $('AXCancel'));
		}
		const menu = axKids(icon())[0];
		return menu ? axKids(menu) : [];
	};
	const frontPid = () => $.NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier;
	const dialog = () => { const wins = axGet(app, 'AXWindows'); for (let i = 0; wins && i < wins.count; i++) { const w = wins.objectAtIndex(i); if (axStr(w, 'AXSubrole') === 'AXDialog') return w; } return null; };
	// 窓がキーを受ける状態か。WebLAV が前面のアプリになっても、窓が前面の窓になるまで少しかかる
	const dialogReady = () => { const focused = axGet(app, 'AXFocusedWindow'); return frontPid() === args.pid && focused !== null && axStr(focused, 'AXSubrole') === 'AXDialog'; };
	const focusState = () => { const focused = axGet(app, 'AXFocusedWindow'); return 'front=' + frontPid() + ' focused=' + (focused ? axStr(focused, 'AXSubrole') + '/' + axStr(focused, 'AXTitle') : 'none'); };
	const bringDialogFront = () => {
		for (let i = 0; i < 50 && !dialogReady(); i++) {
			if (i % 10 === 0) {
				$.NSRunningApplication.runningApplicationWithProcessIdentifier(args.pid).activateWithOptions($.NSApplicationActivateIgnoringOtherApps);
				const w = dialog();
				if (w) $.AXUIElementPerformAction(w, $('AXRaise'));
			}
			delay(0.1);
		}
		// 前面の窓になった直後のキーは落ちることがある
		delay(0.3);
	};
`;

/**
 * メニューバーのアイコンをアクセシビリティから押す (VoiceOver で開くのと同じ)。出たメニューはすぐ取り消して閉じる。
 * メニューの要素は閉じている間も残るので、出たかは閉じた後の項目で見る
 */
export function pressIcon(pid) {
	return jxa(
		`${AX}
	$.AXUIElementPerformAction(icon(), $('AXPress'));
	delay(1);
	const menu = axKids(icon())[0];
	if (menu) $.AXUIElementPerformAction(menu, $('AXCancel'));`,
		{ pid }
	);
}

/** メニューバーのアイコンの数 */
export function menuBarIconCount(pid) {
	return jxa(`${AX} return axKids(axGet(app, 'AXExtrasMenuBar')).length;`, { pid });
}

/**
 * メニューバーのアイコンのメニューの項目 (区切りは title が空) を読む。メニューは開いたままにしない。
 * 一度押した後は、閉じている間もメニューの要素がアクセシビリティから読める。開いて読む形にしないのは、メニューバーの
 * アイコンが多くて確かめ用のアイコンが画面に入りきらないと出ず、開いている間は WebLAV のイベントループも止まるため
 */
export function readMenu(pid) {
	return jxa(
		`${AX}
	return menuItems().map((m) => ({ title: axStr(m, 'AXTitle') ?? '', enabled: axBool(m, 'AXEnabled'), checked: (axStr(m, 'AXMenuItemMarkChar') ?? '') !== '' }));
`,
		{ pid }
	);
}

/** メニューの名前の項目を押す。メニューは開いたままにしない (`readMenu`) */
export function pressMenuItem(pid, title) {
	return jxa(
		`${AX}
	const item = menuItems().find((m) => axStr(m, 'AXTitle') === args.title);
	if (!item) throw new Error('メニューに項目が無い: ' + args.title);
	$.AXUIElementPerformAction(item, $('AXPress'));
`,
		{ pid, title }
	);
}

/** 前面のアプリのプロセス番号と、WebLAV のフォルダー選択の窓が前面の窓か */
export function frontState(pid) {
	return jxa(
		`${AX} const focused = axGet(app, 'AXFocusedWindow'); return { frontPid: frontPid(), dialogFocused: focused !== null && axStr(focused, 'AXSubrole') === 'AXDialog', dialog: dialog() !== null };`,
		{ pid }
	);
}

/** ほかのアプリ (Finder) を前面にする。ブラウザーで操作している状態の代わり */
export function activateFinder() {
	return jxa(`Application('Finder').activate(); delay(0.5); return true;`);
}

/**
 * 出ているフォルダー選択の窓で、場所を打って選ぶ (「フォルダへ移動」の Cmd+Shift+G)。
 * 窓の中身は別のプロセス (openAndSavePanelService) が描くのでボタンは読めず、キーで操作する。
 * キーがほかのアプリへ入らないよう、打つたびに WebLAV が前面かを確かめ、外れていたら打たずに落とす
 */
export function chooseInDialog(pid, folder) {
	return jxa(
		`${AX}
	const se = Application('System Events');
	if (!dialog()) throw new Error('フォルダー選択の窓が無い');
	bringDialogFront();
	const mine = () => frontPid() === args.pid;
	const guard = (step) => { if (!mine()) throw new Error(step + ': WebLAV が前面でないので打たない'); };
	if (!dialogReady()) throw new Error('フォルダー選択の窓が前面にならないので打たない (' + focusState() + ')');
	// 日本語の入力ソースのままだと、打った場所が変換に入って移れない。打つ間だけ英字にして、落ちても戻す
	ObjC.import('Carbon');
	const inputSource = $.TISCopyCurrentKeyboardInputSource();
	try {
		$.TISSelectInputSource($.TISCopyCurrentASCIICapableKeyboardInputSource());
		delay(0.3);
		guard('始める前');
		se.keystroke('g', { using: ['command down', 'shift down'] });
		delay(1);
		// 打つ途中で前面が替わっても残りがほかのアプリへ入らないよう、区切って打ち、そのつど確かめる
		for (let i = 0; i < args.folder.length; i += 8) {
			guard('場所を打つ前');
			se.keystroke(args.folder.slice(i, i + 8));
		}
		delay(0.5);
		guard('移動する前');
		se.keyCode(36);
		delay(1);
		guard('選ぶ前');
		se.keyCode(36);
	} finally {
		$.TISSelectInputSource(inputSource);
	}
	for (let i = 0; i < 50 && dialog(); i++) delay(0.1);
	if (dialog()) throw new Error('窓が閉じない');
`,
		{ pid, folder }
	);
}

/** 出ているフォルダー選択の窓を Esc で閉じる (取り消し) */
export function cancelDialog(pid) {
	return jxa(
		`${AX}
	if (!dialog()) return false;
	for (let attempt = 0; attempt < 3 && dialog(); attempt++) {
		bringDialogFront();
		if (!dialogReady()) throw new Error('フォルダー選択の窓が前面にならないので打たない (' + focusState() + ')');
		Application('System Events').keyCode(53);
		for (let i = 0; i < 30 && dialog(); i++) delay(0.1);
	}
	if (dialog()) throw new Error('窓が閉じない');
	return true;
`,
		{ pid }
	);
}

/**
 * システム設定の「ログイン項目」の「ログイン時に開く」の一覧から、アプリを外す (利用者が「−」で外すのと同じ)。
 * 終わったらシステム設定を閉じる
 */
export function removeFromLoginItems(appName) {
	return jxa(
		`${AX_READ}
	// NSWorkspace の動いているアプリの一覧は、この中では新しく開いたものに追いつかないので、pgrep で引く
	const sa = Application.currentApplication();
	sa.includeStandardAdditions = true;
	const settingsPid = () => { try { return Number(sa.doShellScript('pgrep -x "System Settings"').split('\\r')[0]); } catch (e) { return 0; } };
	// 前から開いていたら、利用者のものなので閉じない
	const wasOpen = settingsPid() !== 0;
	$.NSWorkspace.sharedWorkspace.openURL($.NSURL.URLWithString('x-apple.systempreferences:com.apple.LoginItems-Settings.extension'));
	const find = (el, ok, depth = 0) => { if (!el || depth > 25) return null; if (ok(el)) return el; for (const k of axKids(el)) { const f = find(k, ok, depth + 1); if (f) return f; } return null; };
	const names = (el) => ['AXTitle', 'AXDescription', 'AXValue'].map((n) => axStr(el, n));
	const row = (root) => find(root, (e) => axStr(e, 'AXRole') === 'AXRow' && find(e, (c) => names(c).includes(args.appName)));
	try {
		let root = null;
		for (let i = 0; i < 75 && !(root && row(root)); i++) { delay(0.2); const pid = settingsPid(); root = pid ? $.AXUIElementCreateApplication(pid) : null; }
		const target = root && row(root);
		if (!target) throw new Error('ログイン時に開く一覧に ' + args.appName + ' が無い');
		$.AXUIElementSetAttributeValue(target, $('AXSelected'), $.NSNumber.numberWithBool(true));
		delay(0.5);
		const remove = find(root, (e) => axStr(e, 'AXRole') === 'AXButton' && ['削除', 'Remove'].includes(axStr(e, 'AXDescription')));
		if (!remove) throw new Error('一覧の「−」のボタンが無い');
		$.AXUIElementPerformAction(remove, $('AXPress'));
		for (let i = 0; i < 25 && row(root); i++) delay(0.2);
		if (row(root)) throw new Error('一覧から外れない');
	} finally {
		// 閉じられなくても、元の失敗を伝える
		try { if (!wasOpen && settingsPid()) Application('System Settings').quit(); } catch (e) {}
	}`,
		{ appName }
	);
}

/** Finder のごみ箱の中の名前 */
export function trashNames() {
	return jxa(`return Application('Finder').trash.items.name();`);
}

/** OS の優先する言語の一覧の先頭 (トレイの表示言語を決めるもの。→ src/tray/mod.rs の tray_locale) */
export function preferredLanguage() {
	return jxa(
		`ObjC.import('Foundation'); return ObjC.deepUnwrap($.NSLocale.preferredLanguages)[0];`
	);
}

export async function localHostName() {
	const { stdout } = await execFileAsync('scutil', ['--get', 'LocalHostName']);
	return stdout.trim();
}

/** 動いている weblav のプロセス番号 (常用のものも含む) */
export async function weblavPids() {
	try {
		const { stdout } = await execFileAsync('pgrep', ['-x', 'weblav']);
		return stdout.split('\n').filter(Boolean).map(Number);
	} catch {
		return [];
	}
}

async function listeningPid(port) {
	try {
		const { stdout } = await execFileAsync('lsof', ['-nP', `-iTCP:${port}`, '-sTCP:LISTEN', '-t']);
		return Number(stdout.trim().split('\n')[0]) || null;
	} catch {
		return null;
	}
}

export function readLog() {
	return fs
		.readdir(path.join(HOME, 'logs'))
		.then((names) =>
			Promise.all(names.map((name) => fs.readFile(path.join(HOME, 'logs', name), 'utf8')))
		)
		.then((texts) => texts.join(''))
		.catch(() => '');
}

/** `open` で確かめ用の WebLAV を起動する (Finder やログイン項目から開くのと同じ LaunchServices を通る) */
export function openApp() {
	return execFileAsync('open', ['-n', '-g', '--env', `WEBLAV_HOME=${HOME}`, APP]);
}

/**
 * 確かめ用の WebLAV を起動し、応答できるまで待つ。`fresh` ならデータを消してから始める。
 * 返すのは、動いているもの (プロセス番号・実際のポート・API)
 */
export async function launch({ fresh = true, port = PORT } = {}) {
	// 前に落ちた回のものが残っていれば、同じデータの置き場を使うので先に止める
	await stopLeftover();
	if (fresh) await fs.rm(HOME, { recursive: true, force: true });
	await fs.mkdir(HOME, { recursive: true });
	await fs.writeFile(path.join(HOME, 'config.toml'), `[server]\nport = ${port}\n`);
	await fs.rm(path.join(HOME, 'weblav.addr'), { force: true });
	await openApp();
	try {
		return await waitReady();
	} catch (error) {
		await stopLeftover();
		throw error;
	}
}

async function waitReady() {
	const addr = await waitFor(
		() => fs.readFile(path.join(HOME, 'weblav.addr'), 'utf8').catch(() => null),
		(text) => text !== null && text.includes(':'),
		{ label: 'WebLAV が起動してアドレスを書く', timeout: 30_000 }
	);
	const actualPort = Number(addr.trim().split(':').pop());
	const pid = await waitFor(() => listeningPid(actualPort), Boolean, {
		label: '待ち受けているプロセス'
	});
	const api = new Api(actualPort);
	await waitFor(
		() => api.status('GET', '/setup/status').catch(() => 0),
		(code) => code > 0,
		{
			label: 'API が応える'
		}
	);
	// メニューバーのアイコンが出るまで (イベントループが回り始めるまで)
	await waitFor(
		() => menuBarIconCount(pid).catch(() => 0),
		(n) => n > 0,
		{ label: 'メニューバーのアイコンが出る' }
	);
	return { pid, port: actualPort, api };
}

/** 確かめ用のデータの置き場を使っている WebLAV (二重起動の鍵を持つもの) を止める */
async function stopLeftover() {
	let pids = [];
	try {
		const { stdout } = await execFileAsync('lsof', ['-t', path.join(HOME, 'weblav.lock')]);
		pids = stdout.split('\n').filter(Boolean).map(Number);
	} catch {
		return;
	}
	const weblav = await weblavPids();
	for (const pid of pids.filter((pid) => weblav.includes(pid))) await quit({ pid });
}

/** 確かめ用の WebLAV を止めて、データを残したまま起動し直し、ログインした状態にする */
export async function relaunch(running) {
	await quit(running);
	const next = await launch({ fresh: false });
	try {
		await next.api.login();
	} catch (error) {
		await quit(next);
		throw error;
	}
	return next;
}

/** 確かめ用の WebLAV を止め、終わるまで待つ */
export async function quit(running) {
	if (!running) return;
	try {
		process.kill(running.pid, 'SIGTERM');
	} catch {
		return;
	}
	const gone = () => weblavPids().then((pids) => !pids.includes(running.pid));
	try {
		await waitFor(gone, Boolean, { label: 'WebLAV が終わる' });
		return;
	} catch {
		// 止まらなければ (メニューや窓が開いたままなど)、強制終了する。鍵とポートを持ったままだと、次の起動が通らない
	}
	try {
		process.kill(running.pid, 'SIGKILL');
	} catch {
		return; // 待ちの切れた直後に終わった
	}
	await waitFor(gone, Boolean, { label: 'WebLAV が強制終了で終わる' });
}

/** クッキーを持ち回る、確かめ用の WebLAV の API */
export class Api {
	constructor(port) {
		this.port = port;
		this.origin = `http://localhost:${port}`;
		this.cookie = '';
	}

	async request(method, url, { json, form } = {}) {
		const headers = { origin: this.origin };
		if (this.cookie) headers.cookie = this.cookie;
		let body;
		if (json !== undefined) {
			headers['content-type'] = 'application/json';
			body = JSON.stringify(json);
		} else if (form) body = form;
		const response = await fetch(`${this.origin}/api/v1${url}`, { method, headers, body });
		const set = response.headers.getSetCookie?.() ?? [];
		if (set.length > 0) this.cookie = set.map((c) => c.split(';')[0]).join('; ');
		return response;
	}

	async status(method, url, options) {
		return (await this.request(method, url, options)).status;
	}

	async json(method, url, options) {
		const response = await this.request(method, url, options);
		const text = await response.text();
		if (!response.ok) throw new Error(`${method} ${url}: ${response.status} ${text}`);
		return text ? JSON.parse(text) : null;
	}

	/** 最初の管理者を作り、ログインした状態にする */
	async createAdmin() {
		const token = await (await this.request('POST', '/setup/token')).text();
		await this.json('POST', '/setup/admin', {
			json: { token, username: 'admin', password: PASSWORD }
		});
	}

	login() {
		return this.json('POST', '/auth/login', { json: { username: 'admin', password: PASSWORD } });
	}

	/** フォルダー選択の窓を出させる。窓が閉じると、選ばれたパス (取り消しは null) で解決する */
	pick() {
		return this.json('POST', '/admin/roots/pick').then((body) => body.path);
	}

	addRoot(folderPath) {
		return this.json('POST', '/admin/roots', { json: { path: folderPath } });
	}

	browse(shareId, relative = '') {
		return this.json('GET', `/shares/${shareId}/browse?path=${encodeURIComponent(relative)}`);
	}

	/** 処理 (削除・移動など) が終わるまで待ち、終わったときの進み具合を返す */
	waitJob(jobId) {
		return waitFor(
			() => this.json('GET', `/shares/jobs/${jobId}`),
			(job) => job.state !== 'running',
			{ label: '処理が終わる', timeout: 30_000 }
		);
	}
}

/** フォルダー選択の窓が出るまで待つ */
export function waitDialog(running) {
	return waitFor(
		() => frontState(running.pid),
		(state) => state.dialog,
		{ label: 'フォルダー選択の窓が出る' }
	);
}

/**
 * フォルダー選択の窓で `folder` を選んで共有に足す。返すのは足した共有。
 * 窓を出させる要求は窓が閉じるまで返らないので、待つ間に窓を操作する
 */
export async function pickAndAddRoot(running, folder) {
	const picked = running.api.pick();
	await waitDialog(running);
	await chooseInDialog(running.pid, folder);
	const chosen = await picked;
	return running.api.addRoot(chosen);
}

/** `ram://` のディスクを作って載せ、載せた場所を返す。別のボリュームへの移動を試すのに使う */
export async function attachRamDisk(name, megabytes = 32) {
	const mount = path.join('/Volumes', name);
	// 前に落ちた回のものが残っていると、新しいほうが別の名前 (`<名前> 1`) で載る
	await execFileAsync('hdiutil', ['detach', mount, '-force']).catch(() => {});
	const { stdout } = await execFileAsync('hdiutil', [
		'attach',
		'-nomount',
		`ram://${megabytes * 2048}`
	]);
	const device = stdout.trim().split(/\s+/)[0];
	try {
		await execFileAsync('diskutil', ['erasevolume', 'APFS', name, device]);
	} catch (error) {
		await execFileAsync('hdiutil', ['detach', device, '-force']).catch(() => {});
		throw error;
	}
	return { device, mount };
}

export async function detachRamDisk(disk) {
	if (disk) await execFileAsync('hdiutil', ['detach', disk.device, '-force']).catch(() => {});
}

/** SMB の共有 (`smb://<ユーザー>@<ホスト>/<共有>`) を載せている場所。載っていなければ null */
export async function smbMountPoint(url) {
	const { stdout } = await execFileAsync('mount');
	const prefix = `${url.replace(/^smb:/, '')} on `;
	const line = stdout.split('\n').find((l) => l.startsWith(prefix));
	return line ? line.slice(prefix.length).replace(/ \([^)]*\)$/, '') : null;
}

/** SMB の共有を載せ、載せた場所を返す。パスワードはキーチェーンから読ませる (無ければ OS が尋ねる) */
export async function mountSmb(url) {
	const mounted = await smbMountPoint(url);
	if (mounted) return mounted;
	await jxa(
		`const app = Application.currentApplication(); app.includeStandardAdditions = true; app.mountVolume(args.url);`,
		{ url }
	);
	return waitFor(() => smbMountPoint(url), Boolean, { label: 'SMB の共有が載る', timeout: 30_000 });
}

/** 載せた共有を取り出す (Finder の「取り出す」と同じ) */
export async function unmount(mount) {
	await execFileAsync('diskutil', ['unmount', mount]);
}

/** 待ち受けているだけのサーバーで、ポートを先に取る */
export function occupyPort(port) {
	return new Promise((resolve, reject) => {
		const child = spawn(process.execPath, [
			'-e',
			`require('node:net').createServer().listen(${port}, '0.0.0.0', () => console.log('ready'))`
		]);
		child.stdout.once('data', () => resolve(child));
		child.once('error', reject);
	});
}

/**
 * ログインした画面のセッション (launchd の Aqua) で動いているかを確かめる。Herdr のサーバーのように launchd から
 * 直に立った処理の下 (Background) では、アクセシビリティの許可が効かず、前面のアプリも読めない
 */
export async function assertGuiSession() {
	const { stdout } = await execFileAsync('launchctl', ['managername']);
	if (stdout.trim() !== 'Aqua')
		throw new Error(
			`ログインした画面のセッションで動いていません (${stdout.trim()})。Terminal などの端末の新しいウィンドウから回してください`
		);
}

/**
 * 画面がロックされていれば、始める前に落とす。ロックの間は前面が loginwindow になり、窓が前面にならないので、
 * フォルダー選択の窓の確認が打たずに落ちる
 */
export async function assertScreenUnlocked() {
	const { stdout } = await execFileAsync('ioreg', ['-n', 'Root', '-d1', '-a']);
	if (/<key>CGSSessionScreenIsLocked<\/key>\s*<true\/>/.test(stdout))
		throw new Error('画面がロックされています。ロックを解いてから回してください');
}

/** 回す端末にアクセシビリティの許可があるか。無いと要素の読み取りが黙って失敗し、原因の分からない時間切れになる */
export async function assertPermissions() {
	const trusted = await jxa(`ObjC.import('ApplicationServices'); return $.AXIsProcessTrusted();`);
	if (!trusted)
		throw new Error(
			'アクセシビリティの許可がありません。システム設定の「プライバシーとセキュリティ」の「アクセシビリティ」で、この確認を流している端末をオンにしてください'
		);
}
