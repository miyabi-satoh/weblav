import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import {
	cancelDialog,
	launch,
	mountSmb,
	pickAndAddRoot,
	quit,
	smbMountPoint,
	unmount,
	waitFor
} from '../lib/macos.mjs';

// NAS (SMB の共有) の中のフォルダーを共有にしたとき (docs/folders.md「NAS と外付けのディスク」)。ごみ箱を使えない場所の
// 判定 (src/api/shares/fsops.rs) と、取り出された共有をブックマークからつなぎ直すこと (src/folder_reach.rs・src/folder_access.rs)。
// 手元の NAS を環境変数 WEBLAV_E2E_SMB (`smb://<ユーザー>@<ホスト>/<共有>`) で指したときだけ回す。共有の直下に
// 確かめ用のフォルダーを作って消す。途中で共有を取り出すので、ほかで使っている最中には回さない

const SMB = process.env.WEBLAV_E2E_SMB;
const names = (browse) => browse.entries.map((entry) => entry.name).sort();

test.describe('macOS: NAS の共有', { skip: !SMB && 'WEBLAV_E2E_SMB が無い' }, () => {
	let running;
	let folderName;
	let share;
	// 回す前に載っていなければ、終わったら取り出して元に戻す
	let wasMounted;

	test.before(async () => {
		wasMounted = (await smbMountPoint(SMB)) !== null;
		const mount = await mountSmb(SMB);
		folderName = `weblav-e2e-${Date.now()}`;
		await fs.mkdir(path.join(mount, folderName));
		await fs.writeFile(path.join(mount, folderName, 'hello.txt'), 'hello');
		running = await launch();
		await running.api.createAdmin();
	});

	test.after(async () => {
		const steps = [
			() => running && cancelDialog(running.pid),
			() => quit(running),
			async () => {
				if (!folderName) return;
				const mount = await mountSmb(SMB);
				await fs.rm(path.join(mount, folderName), { recursive: true, force: true });
			},
			async () => {
				const mount = wasMounted === false && (await smbMountPoint(SMB));
				if (mount) await unmount(mount);
			}
		];
		for (const step of steps)
			await Promise.resolve()
				.then(step)
				.catch((error) => console.error(error));
	});

	test('NAS の中のフォルダーを窓で選ぶと共有になり、中を見られる。ごみ箱は使えないと伝える', async () => {
		share = await pickAndAddRoot(running, path.join(await smbMountPoint(SMB), folderName));
		assert.deepEqual(names(await running.api.browse(share.id)), ['hello.txt']);
		const check = await running.api.json('POST', `/shares/${share.id}/delete/check`, {
			json: { paths: ['hello.txt'] }
		});
		assert.equal(check.trash, false, '消すと元に戻せない');
	});

	test('共有を取り出しても、開くとつなぎ直して見られる', async () => {
		assert.ok(share, '前のテストで共有にしている');
		await unmount(await smbMountPoint(SMB));
		assert.equal(await smbMountPoint(SMB), null, '取り出されている');
		// つなぎ直しは開いたときに頼む。載るまでの間は、つながっていないと返ることがある
		const browse = await waitFor(() => running.api.browse(share.id).catch(() => null), Boolean, {
			label: 'つなぎ直して見られる',
			timeout: 60_000,
			interval: 2000
		});
		assert.deepEqual(names(browse), ['hello.txt']);
		assert.ok(await smbMountPoint(SMB), 'OS が共有を載せ直している');
	});
});
