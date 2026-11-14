import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import {
	activateFinder,
	attachRamDisk,
	cancelDialog,
	chooseInDialog,
	detachRamDisk,
	frontState,
	launch,
	pickAndAddRoot,
	quit,
	trashNames,
	waitFor,
	workDir
} from '../lib/macos.mjs';

// 共有のフォルダーと OS のつながり: フォルダー選択の窓 (src/folder_picker.rs)、サンドボックスの外のフォルダーを
// 次の起動でも読むためのブックマーク (src/folder_access.rs)、OS のごみ箱と、別のボリュームへの移動 (src/api/shares/fsops.rs)

const names = (browse) => browse.entries.map((entry) => entry.name).sort();

test.describe('macOS: 共有のフォルダー', () => {
	let running;
	let folder;
	let share;
	let ramDisk;

	test.before(async () => {
		folder = await workDir('folders');
		await fs.writeFile(path.join(folder, 'hello.txt'), 'hello');
		running = await launch();
		await running.api.createAdmin();
	});

	test.after(async () => {
		await cancelDialog(running.pid).catch(() => {});
		await quit(running);
		await detachRamDisk(ramDisk);
	});

	test('窓を取り消すと、何も選ばれない', async () => {
		const picked = running.api.pick();
		await waitFor(
			() => frontState(running.pid),
			(state) => state.dialog,
			{
				label: 'フォルダー選択の窓が出る'
			}
		);
		await cancelDialog(running.pid);
		assert.equal(await picked, null);
	});

	test('共有の追加で、OS の窓がほかのアプリより前に出て、選んだフォルダーが共有になり中を見られる', async () => {
		// 管理画面を開いているブラウザーの代わりに、Finder を前面にしておく
		await activateFinder();
		const picked = running.api.pick();
		await waitFor(
			() => frontState(running.pid),
			(state) => state.frontPid === running.pid && state.dialogFocused,
			{ label: 'WebLAV が前面になり、フォルダー選択の窓が前面の窓になる' }
		);
		await chooseInDialog(running.pid, folder);
		assert.equal(await picked, folder);
		share = await running.api.addRoot(folder);
		assert.deepEqual(names(await running.api.browse(share.id)), ['hello.txt']);
	});

	test('起動し直しても、選んだフォルダーを読める', async () => {
		await quit(running);
		running = await launch({ fresh: false });
		await running.api.login();
		assert.deepEqual(names(await running.api.browse(share.id)), ['hello.txt']);
	});

	test('バックアップから戻した後も、選んだフォルダーを読める', async () => {
		const { api } = running;
		const { ticket } = await api.json('POST', '/admin/backup/ticket');
		const response = await api.request('GET', `/admin/backup?ticket=${encodeURIComponent(ticket)}`);
		assert.equal(response.status, 200);
		const zip = new Blob([await response.arrayBuffer()], { type: 'application/zip' });
		const form = new FormData();
		form.append('file', zip, 'backup.zip');
		const staged = await api.json('POST', '/admin/backup/restore/stage', { form });
		assert.equal(
			await api.status('POST', '/admin/backup/restore', { json: { id: staged.id } }),
			204
		);
		// 戻すと全員のセッションが消える
		await api.login();
		assert.deepEqual(names(await api.browse(share.id)), ['hello.txt']);
	});

	test('共有の中で消すと、OS のごみ箱に入る', async () => {
		const name = `weblav-e2e-${Date.now()}.txt`;
		await fs.writeFile(path.join(folder, name), 'trash me');
		const { jobId } = await running.api.json('POST', `/shares/${share.id}/delete`, {
			json: { paths: [name], permanent: false }
		});
		const job = await running.api.waitJob(jobId);
		assert.equal(job.state, 'succeeded', JSON.stringify(job));
		await fs.access(path.join(folder, name)).then(
			() => assert.fail('消したファイルが残っている'),
			() => {}
		);
		assert.ok((await trashNames()).includes(name), 'ごみ箱に入っている');
	});

	test('別のボリュームの共有へフォルダーを上書きで移動すると、置き先が新しいものに替わり、元が消える', async () => {
		ramDisk = await attachRamDisk('WeblavE2E');
		await fs.mkdir(path.join(folder, 'move-me'));
		await fs.writeFile(path.join(folder, 'move-me', 'new.txt'), 'new');
		await fs.mkdir(path.join(ramDisk.mount, 'move-me'));
		await fs.writeFile(path.join(ramDisk.mount, 'move-me', 'old.txt'), 'old');
		const other = await pickAndAddRoot(running, ramDisk.mount);

		const { jobId } = await running.api.json('POST', '/shares/transfer', {
			json: {
				op: 'move',
				from: { shareId: share.id, paths: ['move-me'] },
				to: { shareId: other.id, path: '' },
				onConflict: 'overwrite'
			}
		});
		const job = await running.api.waitJob(jobId);
		assert.equal(job.state, 'succeeded', JSON.stringify(job));
		assert.deepEqual(await fs.readdir(path.join(ramDisk.mount, 'move-me')), ['new.txt']);
		await fs.access(path.join(folder, 'move-me')).then(
			() => assert.fail('移動した元が残っている'),
			() => {}
		);
	});
});
