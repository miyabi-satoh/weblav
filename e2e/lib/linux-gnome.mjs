// linux.mjs の、GNOME (Wayland) での窓とフォーカスの扱い。GNOME には hyprctl のような口が無く、GNOME Shell の Eval も
// 閉じているので、アクセシビリティ (AT-SPI。atspi.py) で読む
import { execFile, spawn } from 'node:child_process';
import path from 'node:path';
import { promisify } from 'node:util';
import { sleep, waitFor } from './common.mjs';

const execFileAsync = promisify(execFile);

const ATSPI = path.join(import.meta.dirname, 'atspi.py');
const OTHER_APP = 'gnome-text-editor';

async function atspi(...args) {
	return (await execFileAsync('python3', [ATSPI, ...args])).stdout;
}

/** GNOME Shell が動いていれば、そのセッションの環境変数を足して true を返す */
export async function detect(runtime) {
	const running = await execFileAsync('pgrep', [
		'-u',
		String(process.getuid()),
		'-x',
		'gnome-shell'
	]).then(
		() => true,
		() => false
	);
	if (!running) return false;
	process.env.WAYLAND_DISPLAY ??= 'wayland-0';
	process.env.DBUS_SESSION_BUS_ADDRESS ??= `unix:path=${runtime}/bus`;
	return true;
}

export const name = 'gnome';
export const tools = [
	['python3', 'sudo apt install python3-gi gir1.2-atspi-2.0'],
	['gnome-text-editor', 'sudo apt install gnome-text-editor']
];

/**
 * 回す前に、画面を消さない・ロックしない設定にして、ロックされていれば解く。VM は手で触らないので、
 * 5 分たつと GNOME が画面を消してロックし、トレイも窓も読めなくなるため
 */
export async function preflight() {
	await execFileAsync('gsettings', ['set', 'org.gnome.desktop.session', 'idle-delay', '0']);
	await execFileAsync('gsettings', [
		'set',
		'org.gnome.desktop.screensaver',
		'lock-enabled',
		'false'
	]);
	await execFileAsync('loginctl', ['unlock-sessions']).catch(() => {});
}

/** 見えている窓。`app` は AT-SPI のアプリ名 */
async function windows() {
	return JSON.parse(await atspi('windows')).filter((w) => w.showing);
}

export async function activeWindow() {
	const active = (await windows()).find((w) => w.active);
	return active ? { app: active.app, title: active.title } : null;
}

/** テキストエディターを開く。新しい窓は前面に出る */
export async function launchOtherWindow() {
	const child = spawn('gnome-text-editor', ['--new-window'], {
		stdio: 'ignore',
		detached: true
	});
	child.unref();
	await waitFor(activeWindow, (w) => w?.app === OTHER_APP, {
		timeout: 15_000,
		label: 'ほかの窓が開いて前面に出る'
	});
	return {
		app: OTHER_APP,
		async close() {
			await execFileAsync('pkill', ['-x', 'gnome-text-edit']).catch(() => {});
		}
	};
}

/**
 * GNOME 47 からの窓 (Nautilus) は、場所を打った Enter でそのフォルダーの中へ移るだけなので、続けて「Select」を押して
 * 今いるフォルダーに決める。押すのは、Enter の後も窓が前面に残っているときだけ (決まって閉じる窓もあるため)
 */
export async function confirmFolder(dialog) {
	await sleep(1500);
	// 中へ移ると窓の題がフォルダーの名前に変わるので、アプリで見る
	if ((await activeWindow())?.app !== dialog.app) return;
	await atspi('press', 'Select');
}
