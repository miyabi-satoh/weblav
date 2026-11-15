import test from 'node:test';
import assert from 'node:assert/strict';
import {
	PORT,
	STARTUP_TASK_STATE,
	cleanUp,
	installFresh,
	launch,
	occupyPort,
	openApp,
	packagePids,
	quit,
	readLog,
	readStartupTaskState,
	removePackage,
	sleep,
	uiLanguageId,
	waitFor,
	writeStartupTaskState
} from '../lib/windows.mjs';
import {
	clickTrayMenuItem,
	closeLeftoverTrayMenu,
	closeTrayMenu,
	closeTrayOverflow,
	countTrayIcons,
	openTrayMenu,
	readTrayMenu
} from '../lib/windows-tray.mjs';
import { labelsForLangId } from '../lib/tray-labels.mjs';

// タスクトレイ (src/tray/)・サインイン時に起動 (src/tray/login_item.rs の StartupTask)・多重起動 (src/single_instance.rs)。
// 文言は OS の表示言語で出る。メニューの項目は本物のマウスで押す

// 2つ目の起動が、動いているほうの画面を開いたときのログ (src/tray/mod.rs の open_in_browser)
const OPENED = 'opened the running instance in the browser';
const texts = (items) => items.map((item) => (item.separator ? '' : item.text));

/** メニューを出して読み、閉じる */
async function menuOnce(pid) {
	const { hwnd } = await openTrayMenu(pid);
	try {
		return await readTrayMenu(hwnd);
	} finally {
		await closeTrayMenu(pid);
	}
}

/**
 * 読んだメニューが `ok` を満たすまで、出し直して読む。メニューの中身は、アイコンを押したときや管理者の有無の
 * 問い合わせ (3秒ごと) の後に替わり、出ている間には替わらないことがあるため
 */
function menuUntil(pid, ok, label) {
	return waitFor(() => menuOnce(pid), ok, { label, timeout: 20_000, interval: 500 });
}

test.describe('MSIX 版: トレイ', () => {
	let labels;
	let pkg;
	let running;

	test.before(async () => {
		labels = labelsForLangId(await uiLanguageId());
		pkg = await installFresh();
	});

	test.afterEach(async () => {
		await cleanUp([
			() => running && closeLeftoverTrayMenu(running.pid),
			() => closeTrayOverflow(),
			() => quit(running)
		]);
		running = null;
	});

	test.after(async () => {
		await cleanUp([() => removePackage()]);
	});

	test('管理者がいないうちは先頭に「セットアップ」が出て、作ると消える。文言は OS の表示言語で出る', async () => {
		running = await launch(pkg);
		const before = await menuUntil(
			running.pid,
			(items) => items[0]?.text === labels.setup,
			'「セットアップ」が出る'
		);
		assert.deepEqual(texts(before), [
			labels.setup,
			labels.open,
			labels.manual,
			'',
			labels.signInItem,
			'',
			labels.quit
		]);
		await running.api.createAdmin();
		await menuUntil(
			running.pid,
			(items) => !texts(items).includes(labels.setup),
			'管理者を作ると「セットアップ」が消える'
		);
	});

	test('アイコンは1つだけ出る', async () => {
		running = await launch(pkg);
		await menuOnce(running.pid);
		assert.equal(await countTrayIcons(), 1);
	});

	test('設定のポートが使われていると次の番号で動き、メニューの先頭に押せない項目でそう出る', async () => {
		const configured = PORT + 10;
		const blocker = await occupyPort(configured);
		try {
			running = await launch(pkg, { port: configured });
			assert.equal(running.port, configured + 1);
			const items = await menuOnce(running.pid);
			assert.equal(items[0].text, labels.portMoved(configured, configured + 1));
			assert.equal(items[0].enabled, false, 'ポートの知らせは押せない');
			assert.equal(items[1].separator, true, '知らせの下に区切りがある');
		} finally {
			blocker.kill();
		}
	});

	test('「サインイン時に起動」を押すと StartupTask が切り替わり、チェックが付け外しされる', async () => {
		running = await launch(pkg);
		const initial = await menuUntil(
			running.pid,
			(items) => items.some((item) => item.text === labels.signInItem),
			'「サインイン時に起動」が出る'
		);
		const wasOn = initial.find((item) => item.text === labels.signInItem).checked;
		for (const expectOn of [!wasOn, wasOn]) {
			const { hwnd } = await openTrayMenu(running.pid);
			await clickTrayMenuItem(hwnd, labels.signInItem);
			await waitFor(
				() => readStartupTaskState(pkg),
				(state) => state === (expectOn ? STARTUP_TASK_STATE.enabled : STARTUP_TASK_STATE.disabled),
				{ label: `StartupTask が${expectOn ? '有効' : '無効'}になる` }
			);
			await menuUntil(
				running.pid,
				(items) => items.find((item) => item.text === labels.signInItem)?.checked === expectOn,
				`チェックが${expectOn ? '付く' : '外れる'}`
			);
		}
	});

	test('「設定」のスタートアップ アプリで止めると、項目が押せなくなり、止められている旨が付く', async () => {
		running = await launch(pkg);
		await writeStartupTaskState(pkg, STARTUP_TASK_STATE.disabledByUser);
		const items = await menuUntil(
			running.pid,
			(found) => found.some((item) => item.text === labels.signInItemLocked),
			'止められている旨が出る'
		);
		const item = items.find((found) => found.text === labels.signInItemLocked);
		assert.equal(item.enabled, false, '押せない');
		assert.equal(item.checked, false);
		await writeStartupTaskState(pkg, STARTUP_TASK_STATE.disabled);
	});

	test('動いている間にもう一度起動すると、2つ目は動いているほうの画面を開いてすぐ終わり、プロセスもアイコンも増えない', async () => {
		running = await launch(pkg);
		await running.api.createAdmin();
		const count = (text, needle) => text.split(needle).length - 1;
		const logBefore = await readLog(pkg);
		const openedBefore = count(logBefore, OPENED);
		await openApp(pkg);
		const log = await waitFor(
			() => readLog(pkg),
			(text) => count(text, OPENED) > openedBefore,
			{ label: '2つ目が、動いているほうの画面を開く', timeout: 30_000 }
		);
		// 管理者がいるので、セットアップでなくホームを開く
		const opened = log.split(/\r?\n/).findLast((line) => line.includes(OPENED));
		assert.match(opened, new RegExp(`url="?http://localhost:${running.port}(?![\\d/])`));
		await waitFor(
			() => packagePids(pkg),
			(pids) => pids.length === 1 && pids[0] === running.pid,
			{ label: '2つ目が終わり、動いているほうは残る' }
		);
		// 終わったあとにアイコンが残らないことも見る
		await sleep(1000);
		assert.equal(await countTrayIcons(), 1);
	});

	test('「終了する」でプロセスが終わる', async () => {
		running = await launch(pkg);
		const { hwnd } = await openTrayMenu(running.pid);
		await clickTrayMenuItem(hwnd, labels.quit);
		await waitFor(
			() => packagePids(pkg),
			(pids) => pids.length === 0,
			{ label: '終了する', timeout: 20_000 }
		);
		running = null;
	});
});
