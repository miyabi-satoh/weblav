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
// 既定のブラウザーで開いてすぐ終わる。開いたページは残る (閉じる手段が無い)

const ALREADY_RUNNING = 'another instance is already running';
const count = (text, needle) => text.split(needle).length - 1;

test.describe('macOS: 多重起動', () => {
	let running;

	test.after(async () => {
		await quit(running);
	});

	test('動いている間にもう一度起動すると、2つ目はすぐ終わり、プロセスもメニューバーのアイコンも増えない', async () => {
		running = await launch();
		await running.api.createAdmin();
		const pidsBefore = await weblavPids();
		const logBefore = count(await readLog(), ALREADY_RUNNING);

		await openApp();
		await waitFor(
			async () => count(await readLog(), ALREADY_RUNNING),
			(n) => n > logBefore,
			{ label: '2つ目が、動いているほうがあると気づく' }
		);
		const pidsAfter = await waitFor(
			() => weblavPids(),
			(pids) => pids.length === pidsBefore.length,
			{ label: '2つ目が終わる' }
		);
		assert.deepEqual(pidsAfter.sort(), pidsBefore.sort());
		// 終わったあとにアイコンが残らないことも見る
		await sleep(1000);
		assert.equal(await menuBarIconCount(running.pid), 1);
	});
});
