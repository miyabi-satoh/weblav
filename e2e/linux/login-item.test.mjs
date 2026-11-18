import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import {
	APP,
	AUTOSTART_FILE,
	cleanUp,
	launch,
	pressMenuItem,
	quit,
	readMenu,
	waitFor
} from '../lib/linux.mjs';
import { TRAY_LABELS } from '../lib/tray-labels.mjs';

// ログイン時に起動 (src/tray/login_item.rs)。XDG Autostart の .desktop (~/.config/autostart/weblav.desktop) を書く・消す。
// デスクトップの設定で止めた印 (Hidden=true) の付いたものは、オフとして出す

const labels = TRAY_LABELS.ja;

async function loginItem(pid) {
	return waitFor(
		async () => (await readMenu(pid)).find((item) => item.title === labels.loginItem),
		Boolean,
		{ label: '「ログイン時に起動」が出る' }
	);
}

test.describe('Linux: ログイン時に起動', () => {
	let running;
	let held;

	test.before(async () => {
		held = await fs.readFile(AUTOSTART_FILE, 'utf8').catch(() => null);
		await fs.rm(AUTOSTART_FILE, { force: true });
	});

	test.afterEach(async () => {
		await quit(running);
		running = null;
	});

	test.after(async () => {
		await cleanUp([
			async () => {
				if (held === null) await fs.rm(AUTOSTART_FILE, { force: true });
				else await fs.writeFile(AUTOSTART_FILE, held);
			}
		]);
	});

	test('押すと、起動する実行ファイルを指す .desktop を置き、もう一度押すと消す', async () => {
		running = await launch();
		assert.equal((await loginItem(running.pid)).checked, false);

		await pressMenuItem(running.pid, labels.loginItem);
		const text = await waitFor(
			() => fs.readFile(AUTOSTART_FILE, 'utf8').catch(() => null),
			Boolean,
			{ label: '.desktop が置かれる' }
		);
		assert.match(text, new RegExp(`^Exec="${APP}"$`, 'm'));
		await waitFor(
			() => loginItem(running.pid),
			(item) => item.checked,
			{ label: 'オンで出る' }
		);

		await pressMenuItem(running.pid, labels.loginItem);
		await waitFor(
			() =>
				fs.access(AUTOSTART_FILE).then(
					() => true,
					() => false
				),
			(exists) => !exists,
			{ label: '.desktop が消える' }
		);
	});

	test('デスクトップの設定で止めた印 (Hidden=true) があると、起動し直したときにオフで出て、押すとオンに戻す', async () => {
		await fs.mkdir(path.dirname(AUTOSTART_FILE), { recursive: true });
		await fs.writeFile(
			AUTOSTART_FILE,
			`[Desktop Entry]\nType=Application\nName=WebLAV\nExec="${APP}"\nHidden=true\n`
		);
		running = await launch();
		assert.equal((await loginItem(running.pid)).checked, false);

		await pressMenuItem(running.pid, labels.loginItem);
		await waitFor(
			() => fs.readFile(AUTOSTART_FILE, 'utf8'),
			(text) => !text.includes('Hidden=true'),
			{ label: '止めた印の無い .desktop に書き直す' }
		);
		await waitFor(
			() => loginItem(running.pid),
			(item) => item.checked,
			{ label: 'オンで出る' }
		);
	});
});
