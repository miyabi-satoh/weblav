import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import {
	cleanUp,
	entryNames as names,
	fileExists,
	gvfsSmbPath,
	launch,
	mountSmb,
	pickAndAddRoot,
	quit,
	unmountSmb,
	waitFor
} from '../lib/linux.mjs';

// NAS (SMB の共有) の中のフォルダーを共有にしたとき (docs/folders.md「NAS と外付けのディスク」)。ごみ箱を使えない場所の
// 判定 (src/api/shares/fsops.rs) と、取り外された共有を gio でつなぎ直すこと (src/folder_reach.rs)。
// 共有は、VM の中の samba が guest で出す `e2e` (e2e/vm/user-data-*.yaml)。ファイルの窓の「他の場所」からつなぐのと同じ gvfs で載せる

const HOST = 'localhost';
const SHARE = 'e2e';
const URL = `smb://${HOST}/${SHARE}`;
const MOUNT = () => gvfsSmbPath(HOST, SHARE);

test.describe('Linux: NAS の共有', () => {
	let running;
	let folderName;
	let share;

	test.before(async () => {
		await mountSmb(URL);
		// 前からあるものを後片付けで消さないよう、作れた新しいフォルダーの名前だけを持つ
		folderName = path.basename(await fs.mkdtemp(path.join(MOUNT(), 'weblav-e2e-')));
		await fs.writeFile(path.join(MOUNT(), folderName, 'hello.txt'), 'hello');
		running = await launch();
		await running.api.createAdmin();
	});

	test.after(async () => {
		await cleanUp([
			() => quit(running),
			async () => {
				if (!folderName) return;
				await mountSmb(URL);
				await fs.rm(path.join(MOUNT(), folderName), {
					recursive: true,
					force: true
				});
			},
			() => unmountSmb(URL)
		]);
	});

	test('NAS の中のフォルダーを窓で選ぶと共有になり、中を見られる。ごみ箱は使えないと伝える', async () => {
		share = await pickAndAddRoot(running, path.join(MOUNT(), folderName));
		assert.deepEqual(names(await running.api.browse(share.id)), ['hello.txt']);
		const check = await running.api.json('POST', `/shares/${share.id}/delete/check`, {
			json: { paths: ['hello.txt'] }
		});
		assert.equal(check.trash, false, '消すと元に戻せない');
	});

	test('共有を取り外しても、開くとつなぎ直して見られる', async () => {
		assert.ok(share, '前のテストで共有にしている');
		await unmountSmb(URL);
		assert.equal(await fileExists(path.join(MOUNT(), folderName)), false, '取り外されている');
		// つなぎ直しは開いたときに頼む。載るまでの間は、つながっていないと返ることがある
		const browse = await waitFor(() => running.api.browse(share.id).catch(() => null), Boolean, {
			label: 'つなぎ直して見られる',
			timeout: 60_000,
			interval: 2000
		});
		assert.deepEqual(names(browse), ['hello.txt']);
	});
});
