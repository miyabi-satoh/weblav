import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import {
	activeWindow,
	assertMissing,
	cancelDialog,
	chooseInDialog,
	cleanUp,
	entryNames as names,
	launch,
	launchOtherWindow,
	quit,
	trashedPaths,
	waitDialog,
	waitFor,
	workDir
} from '../lib/linux.mjs';

// 共有のフォルダーと OS のつながり: フォルダー選択の窓 (src/folder_picker.rs。rfd が portal の FileChooser で出す) と、
// OS のごみ箱 (src/api/shares/fsops.rs。freedesktop のごみ箱)

test.describe('Linux: 共有のフォルダー', () => {
	let running;
	let folder;
	let share;
	let other;

	test.before(async () => {
		folder = await workDir('folders');
		await fs.writeFile(path.join(folder, 'hello.txt'), 'hello');
		running = await launch();
		await running.api.createAdmin();
	});

	test.after(async () => {
		await cleanUp([
			() => quit(running),
			() => other?.close(),
			() => folder && fs.rm(folder, { recursive: true, force: true })
		]);
	});

	test('共有の追加で、OS の窓がほかのアプリより前に出て、選んだフォルダーが共有になり中を見られる', async () => {
		// 管理画面を開いているブラウザーの代わりに、ほかのアプリの窓を前面にしておく
		other = await launchOtherWindow();
		const before = await activeWindow();
		const picked = running.api.pick();
		const dialog = await waitDialog(before);
		assert.notEqual(dialog.app, other.app, 'フォルダー選択の窓が前面にある');
		// 打った場所が窓に届いて選ばれることで、窓がキーを受ける前面にあることも見る
		await chooseInDialog(folder);
		assert.equal(await picked, folder);
		share = await running.api.addRoot(folder);
		assert.deepEqual(names(await running.api.browse(share.id)), ['hello.txt']);
	});

	test('窓を取り消すと、何も選ばれない', async () => {
		const before = await activeWindow();
		const picked = running.api.pick();
		await waitDialog(before ?? { app: null, title: null });
		await cancelDialog();
		assert.equal(await picked, null);
	});

	test('共有の中で消すと、OS のごみ箱に入る', async () => {
		assert.ok(share, '前のテストで共有にしている');
		const name = `weblav-e2e-${Date.now()}.txt`;
		await fs.writeFile(path.join(folder, name), 'trash me');
		const { jobId } = await running.api.json('POST', `/shares/${share.id}/delete`, {
			json: { paths: [name], permanent: false }
		});
		const job = await running.api.waitJob(jobId);
		assert.equal(job.state, 'succeeded', JSON.stringify(job));
		await assertMissing(path.join(folder, name), '消したファイルが残っている');
		assert.ok((await trashedPaths()).includes(path.join(folder, name)), 'ごみ箱に入っている');
	});

	test('共有の中で上書きすると、今あったものが OS のごみ箱に入る', async () => {
		assert.ok(share, '前のテストで共有にしている');
		const name = `weblav-e2e-overwritten-${Date.now()}.txt`;
		await fs.writeFile(path.join(folder, name), 'new');
		await fs.mkdir(path.join(folder, 'overwrite'), { recursive: true });
		await fs.writeFile(path.join(folder, 'overwrite', name), 'old');
		const { jobId } = await running.api.json('POST', '/shares/transfer', {
			json: {
				op: 'copy',
				from: { shareId: share.id, paths: [name] },
				to: { shareId: share.id, path: 'overwrite' },
				onConflict: 'overwrite'
			}
		});
		const job = await running.api.waitJob(jobId);
		assert.equal(job.state, 'succeeded', JSON.stringify(job));
		assert.equal(await fs.readFile(path.join(folder, 'overwrite', name), 'utf8'), 'new');
		assert.ok(
			(await trashedPaths()).includes(path.join(folder, 'overwrite', name)),
			'上書きで消えたものがごみ箱に入っている'
		);
	});
});
