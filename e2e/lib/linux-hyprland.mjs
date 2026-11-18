// linux.mjs の、Hyprland での窓とフォーカスの扱い。hyprctl で読む
import { execFile, spawn } from 'node:child_process';
import fs from 'node:fs/promises';
import path from 'node:path';
import { promisify } from 'node:util';
import { waitFor } from './common.mjs';

const execFileAsync = promisify(execFile);

const OTHER_APP = 'weblav-e2e-other';

async function hyprctlJson(what) {
	const { stdout } = await execFileAsync('hyprctl', ['-j', what]);
	return JSON.parse(stdout);
}

/** Hyprland が動いていれば、その環境変数を足して true を返す */
export async function detect(runtime) {
	if (!process.env.HYPRLAND_INSTANCE_SIGNATURE) {
		const instances = await fs.readdir(path.join(runtime, 'hypr')).catch(() => []);
		if (instances.length === 0) return false;
		if (instances.length > 1) {
			throw new Error(
				`Hyprland が ${instances.length} つ動いています。HYPRLAND_INSTANCE_SIGNATURE で選んでください`
			);
		}
		process.env.HYPRLAND_INSTANCE_SIGNATURE = instances[0];
	}
	if (!process.env.WAYLAND_DISPLAY) {
		const socket = (await hyprctlJson('instances')).find(
			(i) => i.instance === process.env.HYPRLAND_INSTANCE_SIGNATURE
		)?.wl_socket;
		if (!socket) throw new Error('Hyprland の Wayland のソケットが分かりません');
		process.env.WAYLAND_DISPLAY = socket;
	}
	return true;
}

export const name = 'hyprland';
export const tools = [
	['hyprctl', 'Hyprland に含まれる'],
	['foot', 'sudo pacman -S foot']
];

export async function activeWindow() {
	const active = await hyprctlJson('activewindow');
	return active?.address ? { app: active.class, title: active.title } : null;
}

/** 端末を1つ開いて、前面に出るまで待つ */
export async function launchOtherWindow() {
	const child = spawn('foot', ['--app-id', OTHER_APP, 'sleep', '600'], {
		stdio: 'ignore',
		detached: true
	});
	child.unref();
	await waitFor(activeWindow, (w) => w?.app === OTHER_APP, {
		timeout: 10_000,
		label: 'ほかの窓が開いて前面に出る'
	});
	return {
		app: OTHER_APP,
		async close() {
			try {
				process.kill(child.pid);
			} catch {
				// 閉じていてもよい
			}
		}
	};
}
