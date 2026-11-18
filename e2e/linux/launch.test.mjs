import test from 'node:test';
import assert from 'node:assert/strict';
import {
	launch,
	openedUrls,
	quit,
	readLog,
	sleep,
	startApp,
	trayIconCount,
	waitFor,
	weblavPids
} from '../lib/linux.mjs';

// 動いている間にもう一度起動したとき (src/single_instance.rs・src/main.rs)。2つ目は、動いているほうの画面を
// 既定のブラウザー (xdg-open) で開いてすぐ終わる。ブラウザーは、開いた URL を書くだけの代わりにしてある (lib/linux.mjs)

const OPENED = 'opened the running instance in the browser';
const count = (text, needle) => text.split(needle).length - 1;

test.describe('Linux: 多重起動', () => {
	let running;

	test.after(async () => {
		await quit(running);
	});

	test('動いている間にもう一度起動すると、2つ目は動いているほうの画面を開いてすぐ終わり、プロセスもトレイのアイコンも増えない', async () => {
		running = await launch();
		await running.api.createAdmin();
		const pidsBefore = await weblavPids();
		const openedBefore = count(await readLog(), OPENED);
		const urlsBefore = (await openedUrls()).length;

		const second = startApp();
		await waitFor(
			async () => count(await readLog(), OPENED),
			(n) => n > openedBefore,
			{ label: '2つ目が、動いているほうの画面を開く' }
		);
		// 管理者がいるので、セットアップでなくホームを開く
		const urls = await waitFor(openedUrls, (list) => list.length > urlsBefore, {
			label: 'ブラウザーに URL が渡る'
		});
		assert.match(urls.at(-1), new RegExp(`^http://localhost:${running.port}(?![\\d/])`));
		await waitFor(
			() => weblavPids(),
			(pids) => !pids.includes(second),
			{ label: '2つ目が終わる' }
		);
		assert.deepEqual(await weblavPids(), pidsBefore, '動いているほうは止まらず、ほかに増えない');
		// 終わったあとにアイコンが残らないことも見る
		await sleep(1000);
		assert.equal(await trayIconCount(running.pid), 1);
	});
});
