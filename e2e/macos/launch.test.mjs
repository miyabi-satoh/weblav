import test from 'node:test';
import assert from 'node:assert/strict';
import {
	launch,
	menuBarIconCount,
	openApp,
	quit,
	readLog,
	sleep,
	waitFor,
	weblavPids
} from '../lib/macos.mjs';

// 動いている間にもう一度起動したとき (src/single_instance.rs・src/main.rs)。2つ目は、動いているほうの画面を
// 既定のブラウザーで開いてすぐ終わる。開いたページは残る (閉じる手段が無い)。開いた先は、2つ目がログに書く

const ALREADY_RUNNING = 'another instance is already running';
const OPENED = 'opened the running instance in the browser';
const count = (text, needle) => text.split(needle).length - 1;

test.describe('macOS: 多重起動', () => {
	let running;

	test.after(async () => {
		await quit(running);
	});

	test('動いている間にもう一度起動すると、2つ目は動いているほうの画面を開いてすぐ終わり、プロセスもメニューバーのアイコンも増えない', async () => {
		running = await launch();
		await running.api.createAdmin();
		const pidsBefore = await weblavPids();
		const logBefore = await readLog();
		const noticedBefore = count(logBefore, ALREADY_RUNNING);
		const openedBefore = count(logBefore, OPENED);

		await openApp();
		await waitFor(
			async () => count(await readLog(), ALREADY_RUNNING),
			(n) => n > noticedBefore,
			{ label: '2つ目が、動いているほうがあると気づく' }
		);
		const log = await waitFor(readLog, (text) => count(text, OPENED) > openedBefore, {
			label: '2つ目が、動いているほうの画面を開く'
		});
		// 管理者がいるので、セットアップでなくホームを開く
		const opened = log.split('\n').findLast((line) => line.includes(OPENED));
		assert.match(opened, new RegExp(`url="?http://localhost:${running.port}(?![\\d/])`));
		// 常用の WebLAV が同じ間に起動・終了しても左右されないよう、新しく増えたプロセスだけを見る
		await waitFor(
			async () => (await weblavPids()).filter((pid) => !pidsBefore.includes(pid)),
			(added) => added.length === 0,
			{ label: '2つ目が終わる' }
		);
		assert.ok((await weblavPids()).includes(running.pid), '動いているほうは止まらない');
		// 終わったあとにアイコンが残らないことも見る
		await sleep(1000);
		assert.equal(await menuBarIconCount(running.pid), 1);
	});
});
