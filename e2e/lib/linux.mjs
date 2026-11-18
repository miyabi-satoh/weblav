// Linux の自動の確認 (`just linux-check-vm`) の土台。OS とつながる所 (トレイ・ログイン時に起動・フォルダー選択の窓・
// ごみ箱・NAS の共有) を、配る .deb で入れた /usr/bin/weblav を VM の中で動かして読む (→ e2e/README.md「Linux の確認」)。
//
// トレイは D-Bus (StatusNotifierItem と dbusmenu)、キーは ydotool (uinput の本物の入力)、ごみ箱と SMB の共有は gio で扱う。
// 窓とフォーカスはデスクトップで違うので、Hyprland は hyprctl (linux-hyprland.mjs)、GNOME は AT-SPI (linux-gnome.mjs) で読む。
//
// 確かめ用の WebLAV は、データの置き場 (`WEBLAV_HOME`) とポートを分けて動かす。
import { execFile, spawn } from 'node:child_process';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';

import { Api, sleep, waitFor } from './common.mjs';
import * as gnome from './linux-gnome.mjs';
import * as hyprland from './linux-hyprland.mjs';

export { Api, PASSWORD, assertMissing, cleanUp, entryNames, sleep, waitFor } from './common.mjs';

const execFileAsync = promisify(execFile);

/** 配る .deb が入れる実行ファイル。ほかのものは環境変数 `WEBLAV_APP` で指す */
export const APP = process.env.WEBLAV_APP ?? '/usr/bin/weblav';
export const HOME = path.join(os.homedir(), 'weblav-e2e');
// 常用のもの (既定 8370) とぶつからない番号
export const PORT = 18370;

const CONFIG_HOME = process.env.XDG_CONFIG_HOME ?? path.join(os.homedir(), '.config');
const DATA_HOME = process.env.XDG_DATA_HOME ?? path.join(os.homedir(), '.local', 'share');
/** ログイン時に起動の .desktop (src/tray/login_item.rs)。データの置き場では分かれない */
export const AUTOSTART_FILE = path.join(CONFIG_HOME, 'autostart', 'weblav.desktop');

const YDOTOOL_SOCKET =
	process.env.YDOTOOL_SOCKET ?? `/run/user/${process.getuid()}/.ydotool_socket`;

/** Linux の入力のキーコード (linux/input-event-codes.h) */
export const KEY = { esc: 1, enter: 28, ctrl: 29, l: 38, alt: 56, home: 102, delete: 111 };

// 2つ目の起動が開くブラウザーの代わり。開いた URL をファイルに書くだけにする。本物のブラウザーを開くと、
// VM の中に窓が残り、続くテストの前面を乱すため。xdg-open (と GNOME の gio open) は mimeapps.list の既定を使う
const BROWSER_ID = 'weblav-e2e-browser.desktop';
export const BROWSER_LOG = path.join(os.tmpdir(), 'weblav-e2e-browser.log');

// ── 環境 ──

/** 回しているデスクトップの扱い (linux-hyprland.mjs・linux-gnome.mjs)。ensureSessionEnv で決まる */
let desktop;

// ログインした画面のセッションが systemd に渡している環境変数のうち、WebLAV のトレイと窓が要るもの
const SESSION_ENV = [
	'WAYLAND_DISPLAY',
	'DISPLAY',
	'XAUTHORITY',
	'XDG_CURRENT_DESKTOP',
	'XDG_SESSION_TYPE',
	'DBUS_SESSION_BUS_ADDRESS'
];

/**
 * 回しているデスクトップを見分け、その扱いを選ぶ。ssh から回すと Wayland などの環境変数が無いので、
 * ログインした画面のものを足す。どちらでもなければ投げる
 */
export async function ensureSessionEnv() {
	if (desktop) return desktop;
	const runtime = process.env.XDG_RUNTIME_DIR ?? `/run/user/${process.getuid()}`;
	process.env.XDG_RUNTIME_DIR = runtime;
	const { stdout } = await execFileAsync('systemctl', ['--user', 'show-environment']).catch(() => ({
		stdout: ''
	}));
	for (const line of stdout.split('\n')) {
		const eq = line.indexOf('=');
		const name = line.slice(0, eq);
		if (eq > 0 && SESSION_ENV.includes(name) && !process.env[name]) {
			process.env[name] = line.slice(eq + 1);
		}
	}
	for (const candidate of [hyprland, gnome]) {
		if (await candidate.detect(runtime)) {
			desktop = candidate;
			break;
		}
	}
	if (!desktop) throw new Error('Hyprland か GNOME の上で回してください (どちらも動いていません)');
	process.env.YDOTOOL_SOCKET = YDOTOOL_SOCKET;
	return desktop;
}

/** 回しているデスクトップの名前 (`hyprland`・`gnome`) */
export function desktopName() {
	return desktop?.name;
}

/** 回す前に、要る道具と状態を確かめ、ブラウザーの代わりを既定にする。足りなければ、直し方を書いて投げる */
export async function preflight() {
	await ensureSessionEnv();
	const missing = [];
	for (const [command, how] of [
		['ydotool', 'ydotool を入れる'],
		['busctl', 'systemd に含まれる'],
		['gio', 'glib に含まれる'],
		['xdg-mime', 'xdg-utils を入れる'],
		...desktop.tools
	]) {
		if (!(await which(command))) missing.push(`${command} (${how})`);
	}
	if (!(await fileExists(APP))) missing.push(`${APP} (配る .deb を入れる)`);
	if (missing.length > 0) throw new Error(`道具が足りません: ${missing.join('、')}`);
	if (!(await fileExists(YDOTOOL_SOCKET))) {
		throw new Error(`ydotoold が動いていません (${YDOTOOL_SOCKET} がありません)`);
	}
	await desktop.preflight();
	await installBrowserStandIn();
}

async function installBrowserStandIn() {
	const dir = path.join(DATA_HOME, 'applications');
	await fs.mkdir(dir, { recursive: true });
	// .desktop の Exec の引用の決まりを避けるため、書く処理は別のスクリプトに置く
	const script = path.join(dir, 'weblav-e2e-browser.sh');
	await fs.writeFile(script, `#!/bin/sh\nprintf '%s\\n' "$1" >> '${BROWSER_LOG}'\n`, {
		mode: 0o755
	});
	await fs.writeFile(
		path.join(dir, BROWSER_ID),
		`[Desktop Entry]\nType=Application\nName=WebLAV e2e browser\nExec=${script} %u\nMimeType=x-scheme-handler/http;x-scheme-handler/https;\nNoDisplay=true\n`
	);
	await execFileAsync('xdg-mime', ['default', BROWSER_ID, 'x-scheme-handler/http']);
	await execFileAsync('xdg-mime', ['default', BROWSER_ID, 'x-scheme-handler/https']);
}

/** ブラウザーの代わりが受けた URL */
export function openedUrls() {
	return fs
		.readFile(BROWSER_LOG, 'utf8')
		.then((text) => text.split('\n').filter(Boolean))
		.catch(() => []);
}

/** PATH から実行ファイルを探す (`which` が入っていない環境があるので自分で探す)。無ければ null */
async function which(command) {
	for (const dir of (process.env.PATH ?? '').split(path.delimiter)) {
		const file = path.join(dir, command);
		if (
			await fs.access(file, fs.constants.X_OK).then(
				() => true,
				() => false
			)
		)
			return file;
	}
	return null;
}

export function fileExists(file) {
	return fs.access(file).then(
		() => true,
		() => false
	);
}

// ── 窓とフォーカス (デスクトップごと) ──

/** フォーカスのある窓 (`{ app, title }`)。無ければ null */
export async function activeWindow() {
	return (await ensureSessionEnv()).activeWindow();
}

/**
 * フォルダー選択の窓が手前に出る前に前面にあった窓として、ほかのアプリの窓を1つ開いて前面にする。
 * 返すものの `app` は activeWindow の `app` と比べる名前、`close` で閉じる
 */
export async function launchOtherWindow() {
	return (await ensureSessionEnv()).launchOtherWindow();
}

// ── ydotool ──

function ydotool(args) {
	return execFileAsync('ydotool', args, {
		env: { ...process.env, YDOTOOL_SOCKET }
	});
}

/** キーを押して離す。`codes` は同時に押す組み合わせ (押した逆の順に離す) */
export async function pressKeys(...codes) {
	const down = codes.map((code) => `${code}:1`);
	const up = [...codes].reverse().map((code) => `${code}:0`);
	await ydotool(['key', '--key-delay', '30', ...down, ...up]);
}

/** ASCII の文字を打つ */
export async function typeText(text) {
	await ydotool(['type', '--key-delay', '20', text]);
}

// ── トレイ (StatusNotifierItem と dbusmenu) ──

async function busctl(args) {
	// `--` の後は引数として読ませる (GetLayout の -1 をオプションと読まれないよう)
	return (await execFileAsync('busctl', ['--user', '--json=short', '--', ...args])).stdout;
}

/** プロセス `pid` が出しているトレイのアイコンの、D-Bus の名前とメニューのパス */
async function trayItems(pid) {
	const items = JSON.parse(
		await busctl([
			'get-property',
			'org.kde.StatusNotifierWatcher',
			'/StatusNotifierWatcher',
			'org.kde.StatusNotifierWatcher',
			'RegisteredStatusNotifierItems'
		])
	).data;
	const found = [];
	for (const item of items) {
		// `<バス名>/<パス>` (Hyprland のパネル) と `<バス名>@<パス>` (GNOME の AppIndicator の拡張) がある
		const split = item.search(/[@/]/);
		const [bus, object] =
			split < 0
				? [item, '/StatusNotifierItem']
				: [item.slice(0, split), item.slice(item[split] === '@' ? split + 1 : split)];
		const owner = await busctl([
			'call',
			'org.freedesktop.DBus',
			'/org/freedesktop/DBus',
			'org.freedesktop.DBus',
			'GetConnectionUnixProcessID',
			's',
			bus
		])
			.then((out) => JSON.parse(out).data[0])
			.catch(() => null);
		if (owner !== pid) continue;
		const menu = await busctl(['get-property', bus, object, 'org.kde.StatusNotifierItem', 'Menu'])
			.then((out) => JSON.parse(out).data)
			.catch(() => null);
		if (menu) found.push({ bus, object, menu });
	}
	return found;
}

/** プロセス `pid` のトレイのアイコンの数 */
export async function trayIconCount(pid) {
	return (await trayItems(pid)).length;
}

async function trayItem(pid) {
	const [item] = await trayItems(pid);
	if (!item) throw new Error('WebLAV のトレイのアイコンが見つかりません');
	return item;
}

/** dbusmenu の (id, { 項目 }, [子]) の入れ子を、項目の並びにする。区切りは `title: ''` (macOS の確認と同じ形) */
function menuItems(children) {
	return children.map(({ data: [id, props, sub] }) => ({
		id,
		title: props.type?.data === 'separator' ? '' : (props.label?.data ?? '').replaceAll('_', ''),
		enabled: props.enabled?.data ?? true,
		checked: props['toggle-state']?.data === 1,
		children: menuItems(sub ?? [])
	}));
}

async function layout(pid) {
	const item = await trayItem(pid);
	const data = JSON.parse(
		await busctl([
			'call',
			item.bus,
			item.menu,
			'com.canonical.dbusmenu',
			'GetLayout',
			'iias',
			'0',
			'-1',
			'0'
		])
	).data[1];
	return { item, items: menuItems(data[2]) };
}

/** トレイのメニューの項目 (`{ title, enabled, checked, children }`) */
export async function readMenu(pid) {
	return (await layout(pid)).items;
}

/** トレイのメニューの、`parent` の下の項目 */
export async function readSubmenu(pid, parent) {
	const entry = (await readMenu(pid)).find((e) => e.title === parent);
	if (!entry) throw new Error(`トレイのメニューに「${parent}」がありません`);
	return entry.children;
}

async function clickItem(item, id) {
	await busctl([
		'call',
		item.bus,
		item.menu,
		'com.canonical.dbusmenu',
		'Event',
		'isvu',
		String(id),
		'clicked',
		's',
		'',
		'0'
	]);
}

/** トレイのメニューの項目を、名前で押す */
export async function pressMenuItem(pid, title) {
	const { item, items } = await layout(pid);
	const entry = items.find((e) => e.title === title);
	if (!entry) throw new Error(`トレイのメニューに「${title}」がありません`);
	await clickItem(item, entry.id);
}

/** トレイのメニューの、`parent` の下の項目を押す */
export async function pressSubmenuItem(pid, parent, title) {
	const { item, items } = await layout(pid);
	const entry = items.find((e) => e.title === parent)?.children.find((e) => e.title === title);
	if (!entry) throw new Error(`トレイのメニューの「${parent}」に「${title}」がありません`);
	await clickItem(item, entry.id);
}

// ── WebLAV のプロセス ──

/** 動いている weblav のプロセス番号 */
export async function weblavPids() {
	try {
		const { stdout } = await execFileAsync('pgrep', ['-x', 'weblav']);
		return stdout.split('\n').filter(Boolean).map(Number);
	} catch {
		return [];
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

/**
 * 確かめ用の WebLAV を、ログインしたときと同じく画面のセッションの子として起動し、プロセス番号を返す。
 * `locale` はトレイの表示言語を決める `LANG` (src/tray/mod.rs の tray_locale)
 */
export function startApp({ locale = 'ja_JP.UTF-8' } = {}) {
	const env = { ...process.env, WEBLAV_HOME: HOME, LANG: locale };
	delete env.LC_ALL;
	delete env.LC_MESSAGES;
	const child = spawn(APP, [], { stdio: 'ignore', detached: true, env });
	child.unref();
	return child.pid;
}

/**
 * 確かめ用の WebLAV を起動し、応答できるまで待つ。`fresh` ならデータを消してから始める。
 * 返すのは、動いているもの (プロセス番号・実際のポート・API)
 */
export async function launch({ fresh = true, port = PORT, trayLanguage, locale } = {}) {
	// 前に落ちた回のものが残っていれば、同じデータの置き場を使うので先に止める
	await stopLeftover();
	if (fresh) await fs.rm(HOME, { recursive: true, force: true });
	await fs.mkdir(HOME, { recursive: true });
	const language = trayLanguage ? `language = "${trayLanguage}"\n` : '';
	await fs.writeFile(
		path.join(HOME, 'config.toml'),
		`[server]\nport = ${port}\n\n[tray]\nopen_setup_on_launch = false\n${language}`
	);
	await fs.rm(path.join(HOME, 'weblav.addr'), { force: true });
	const pid = startApp({ locale });
	try {
		return await waitReady(pid);
	} catch (error) {
		await quit({ pid });
		throw error;
	}
}

async function waitReady(pid) {
	const addr = await waitFor(
		() => fs.readFile(path.join(HOME, 'weblav.addr'), 'utf8').catch(() => null),
		(text) => text !== null && text.includes(':'),
		{ label: 'WebLAV が起動してアドレスを書く', timeout: 30_000 }
	);
	const port = Number(addr.trim().split(':').pop());
	const api = new Api(port);
	await waitFor(
		() => api.status('GET', '/setup/status').catch(() => 0),
		(code) => code > 0,
		{ label: 'API が応える' }
	);
	// トレイのアイコンが出るまで (イベントループが回り始めるまで)
	await waitFor(
		() => trayIconCount(pid).catch(() => 0),
		(n) => n > 0,
		{ label: 'トレイのアイコンが出る', timeout: 15_000 }
	);
	return { pid, port, api };
}

/** 確かめ用のデータの置き場を使っている WebLAV を止める */
async function stopLeftover() {
	for (const pid of await weblavPids()) {
		const environ = await fs.readFile(`/proc/${pid}/environ`, 'utf8').catch(() => '');
		if (environ.split('\0').includes(`WEBLAV_HOME=${HOME}`)) await quit({ pid });
	}
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
		// 止まらなければ (窓が開いたままなど)、強制終了する。鍵とポートを持ったままだと、次の起動が通らない
	}
	try {
		process.kill(running.pid, 'SIGKILL');
	} catch {
		return; // 待ちの切れた直後に終わった
	}
	await waitFor(gone, Boolean, { label: 'WebLAV が強制終了で終わる' });
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

// ── フォルダー選択の窓 ──

/** 共有にするフォルダーを、毎回新しく作る */
export function workDir(name) {
	return fs.mkdtemp(path.join(os.homedir(), `weblav-e2e-${name}-`));
}

/**
 * フォルダー選択の窓が出て前面になるまで待ち、その窓を返す。`before` は窓を出させる前に前面にあった窓。
 * 窓は WebLAV でなく portal のプロセスが出すので、前面がほかの窓に変わったことで見る
 */
export function waitDialog(before) {
	return waitFor(
		activeWindow,
		(w) => w !== null && (w.app !== before.app || w.title !== before.title),
		// GNOME 47 からの Nautilus の窓は、冷えた状態から出ると、アクセシビリティに出るまで 15 秒より長くかかることがある
		{ label: 'フォルダー選択の窓が前面に出る', timeout: 45_000 }
	);
}

/**
 * 前面のフォルダー選択の窓 (portal の GTK の窓) で、場所の欄 (Ctrl+L) にフォルダーのパスを打って選ぶ。
 * 窓は「最近使用したもの」の一覧で開くことがあり、その一覧では打った場所へ移らないので、先にホーム (Alt+Home) へ移る。
 * 打つと、続きが1つに決まる分 (末尾の `/` や、子のフォルダーが1つならその名前) を補完が選んだ状態で足すので、
 * Delete で消してから Enter で決める。Enter で中へ移るだけの窓 (GNOME の Nautilus) は、続けて「選択」を押す。
 * `dialog` は waitDialog が返した窓
 */
export async function chooseInDialog(folder, dialog) {
	await pressKeys(KEY.alt, KEY.home);
	await sleep(500);
	await pressKeys(KEY.ctrl, KEY.l);
	await sleep(300);
	await typeText(folder);
	// 打ち終えてすぐは、GTK が補完を作り直している最中で、Delete と Enter が落ちる
	await sleep(800);
	await pressKeys(KEY.delete);
	await sleep(300);
	await pressKeys(KEY.enter);
	await (await ensureSessionEnv()).confirmFolder(dialog);
}

/** 前面のフォルダー選択の窓を取り消す */
export function cancelDialog() {
	return pressKeys(KEY.esc);
}

/**
 * フォルダー選択の窓で `folder` を選んで共有に足す。返すのは足した共有。
 * 窓を出させる要求は窓が閉じるまで返らないので、待つ間に窓を操作する
 */
export async function pickAndAddRoot(running, folder) {
	const before = await activeWindow();
	const picked = running.api.pick();
	const dialog = await waitDialog(before ?? { app: null, title: null });
	await chooseInDialog(folder, dialog);
	const chosen = await picked;
	return running.api.addRoot(chosen);
}

// ── ごみ箱と SMB の共有 (gio) ──

/** ごみ箱の中のものの、元の場所 */
export async function trashedPaths() {
	const { stdout } = await execFileAsync('gio', ['trash', '--list']);
	// 1行に「ごみ箱の URI<TAB>元のパス」
	return stdout
		.split('\n')
		.filter(Boolean)
		.map((line) => line.split('\t')[1]);
}

/** gvfs が SMB の共有を載せる場所 (`/run/user/<uid>/gvfs/smb-share:server=<ホスト>,share=<共有>`) */
export function gvfsSmbPath(host, share) {
	return path.join(process.env.XDG_RUNTIME_DIR, 'gvfs', `smb-share:server=${host},share=${share}`);
}

/** SMB の共有をつなぐ (ファイルの窓の「他の場所」からつなぐのと同じ gvfs)。guest で入れる共有だけを相手にする */
export async function mountSmb(url) {
	try {
		await execFileAsync('gio', ['mount', '--anonymous', url], { timeout: 30_000 });
	} catch (error) {
		// 前のテストやつなぎ直しで載っていれば、そのまま使う
		if (!String(error.stderr).includes('already mounted')) throw error;
	}
}

/** つないだ共有を外す (ファイルの窓の「取り外す」と同じ) */
export async function unmountSmb(url) {
	await execFileAsync('gio', ['mount', '--unmount', url], {
		timeout: 30_000
	}).catch(() => {});
}
