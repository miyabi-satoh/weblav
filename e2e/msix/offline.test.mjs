import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import {
	cleanUp,
	entryNames,
	installFresh,
	launch,
	quit,
	relaunch,
	removePackage,
	setRootPath,
	workDir
} from '../lib/windows.mjs';

// 電源の切れた NAS の共有 (src/folder_reach.rs)。返事をしない宛先の UNC パスは、Windows のファイル操作が長く戻らない。
// その間も、手元のフォルダーの共有は待たされずに見られ、NAS の共有は「つながっていません」になることを見る。
// 返事をしない宛先には、例に使うと決められたアドレス (192.0.2.1。RFC 5737) を使う。登録の口は今つながる
// フォルダーしか受け付けないので、手元のフォルダーで登録してから、止めている間に DB でパスを書き換える

const OFFLINE_PATH = '\\\\192.0.2.1\\share\\weblav-e2e';
// 手元のフォルダーを開くのに掛かってよい時間。NAS を待つ時間 (TARGET_WAIT の20秒・OTHERS_WAIT の1秒) より十分短く、
// 待たされていれば必ず越える
const LOCAL_LIMIT_MS = 3000;

async function timed(promise) {
	const started = Date.now();
	const value = await promise;
	return { value, ms: Date.now() - started };
}

test.describe('MSIX 版: 電源の切れた NAS', () => {
	let running;
	let local;
	let nas;
	let localShare;
	let nasShare;

	test.before(async () => {
		local = await workDir('local');
		nas = await workDir('offline');
		await fs.writeFile(path.join(local, 'hello'), 'hello');
		const pkg = await installFresh();
		running = await launch(pkg);
		await running.api.createAdmin();
		localShare = await running.api.addRoot(local);
		nasShare = await running.api.addRoot(nas);
		await quit(running);
		await setRootPath(pkg, nasShare.id, OFFLINE_PATH);
		running = await relaunch(running);
	});

	test.after(async () => {
		await cleanUp([
			() => quit(running),
			() => removePackage(),
			() => local && fs.rm(local, { recursive: true, force: true }),
			() => nas && fs.rm(nas, { recursive: true, force: true })
		]);
	});

	test('NAS の共有を開いて待っている間も、手元のフォルダーの共有は待たされずに見られ、NAS の共有はつながっていないと返る', async () => {
		const opening = timed(running.api.request('GET', `/shares/${nasShare.id}/browse?path=`));
		const browsed = await timed(running.api.browse(localShare.id));
		assert.deepEqual(entryNames(browsed.value), ['hello']);
		assert.ok(browsed.ms < LOCAL_LIMIT_MS, `手元のフォルダーが ${browsed.ms}ms 待たされた`);

		const { value: response } = await opening;
		assert.equal(response.status, 503);
		assert.equal((await response.json()).error.code, 'folder_unreachable');
	});

	test('共有の一覧 (管理画面) で、NAS の共有は「つながっていません」、手元のフォルダーは読めると出る', async () => {
		const roots = await running.api.json('GET', '/admin/roots');
		const status = Object.fromEntries(roots.map((root) => [root.id, root.status]));
		assert.equal(status[nasShare.id], 'unreachable');
		assert.equal(status[localShare.id], 'ready');
	});
});
