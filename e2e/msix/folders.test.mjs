import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import {
	assertMissing,
	cancelDialog,
	chooseInDialog,
	cleanUp,
	entryNames,
	closeWindow,
	folderDialog,
	holdFileOpen,
	installFresh,
	isOnTop,
	launch,
	quit,
	recycleBinNames,
	removePackage,
	visibleWindows,
	waitFor,
	workDir
} from '../lib/windows.mjs';
import { runPowerShell } from '../lib/powershell.mjs';

// 共有のフォルダーと OS のつながり: フォルダー選択の窓 (src/folder_picker.rs の最前面の持ち主)、OS のごみ箱と、
// 使用中のファイルの扱い (src/api/shares/fsops.rs)

/** エクスプローラーの窓を開いて前面に出す (管理画面を開いているブラウザーの代わり)。開いた窓を返す */
async function openExplorer(folder) {
	const before = new Set((await visibleWindows()).map((w) => w.hwnd));
	await runPowerShell('Start-Process explorer.exe -ArgumentList $args[0]', [folder]);
	return waitFor(
		async () =>
			(await visibleWindows()).find(
				(w) => !before.has(w.hwnd) && w.className === 'CabinetWClass'
			) ?? null,
		Boolean,
		{ label: 'エクスプローラーの窓が開く' }
	);
}

test.describe('MSIX 版: 共有のフォルダー', () => {
	let running;
	let folder;
	let share;
	let explorer;

	test.before(async () => {
		folder = await workDir('folders');
		await fs.writeFile(path.join(folder, 'hello'), 'hello');
		running = await launch(await installFresh());
		await running.api.createAdmin();
	});

	test.after(async () => {
		await cleanUp([
			() => running && cancelDialog(running.pid),
			() => explorer && closeWindow(explorer.hwnd),
			() => quit(running),
			() => removePackage(),
			() => folder && fs.rm(folder, { recursive: true, force: true })
		]);
	});

	test('窓を取り消すと、何も選ばれない', async () => {
		const picked = running.api.pick();
		await waitFor(() => folderDialog(running.pid), Boolean, { label: 'フォルダー選択の窓が出る' });
		await cancelDialog(running.pid);
		assert.equal(await picked, null);
	});

	test('共有の追加で、OS の窓がほかのアプリより前に出て、選んだフォルダーが共有になり中を見られる', async () => {
		explorer = await openExplorer(folder);
		const picked = running.api.pick();
		const dialog = await waitFor(() => folderDialog(running.pid), Boolean, {
			label: 'フォルダー選択の窓が出る'
		});
		await waitFor(
			() => isOnTop(dialog.hwnd),
			(top) => top,
			{ label: 'フォルダー選択の窓が、エクスプローラーより前に見えている' }
		);
		await chooseInDialog(dialog.hwnd, folder);
		const chosen = await picked;
		assert.equal(chosen?.toLowerCase(), folder.toLowerCase());
		share = await running.api.addRoot(chosen);
		assert.deepEqual(entryNames(await running.api.browse(share.id)), ['hello']);
		await closeWindow(explorer.hwnd);
		explorer = null;
	});

	test('共有の中で消すと、ごみ箱に入る', async () => {
		assert.ok(share, '前のテストで共有にしている');
		const name = `weblav-e2e-${Date.now()}`;
		await fs.writeFile(path.join(folder, name), 'trash me');
		const { jobId } = await running.api.json('POST', `/shares/${share.id}/delete`, {
			json: { paths: [name], permanent: false }
		});
		const job = await running.api.waitJob(jobId);
		assert.equal(job.state, 'succeeded', JSON.stringify(job));
		await assertMissing(path.join(folder, name), '消したファイルが残っている');
		assert.ok((await recycleBinNames()).includes(name), 'ごみ箱に入っている');
	});

	test('共有の中で上書きすると、今あったものがごみ箱に入る', async () => {
		const name = `weblav-e2e-overwritten-${Date.now()}`;
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
		assert.ok((await recycleBinNames()).includes(name), '上書きで消えたものがごみ箱に入っている');
	});

	test('ほかのアプリが開いているファイルへ移動して上書きすると、使用中と返り、置き先はそのまま残る', async () => {
		const name = `weblav-e2e-in-use-${Date.now()}`;
		await fs.writeFile(path.join(folder, name), 'new');
		await fs.mkdir(path.join(folder, 'in-use'), { recursive: true });
		const target = path.join(folder, 'in-use', name);
		await fs.writeFile(target, 'old');
		const held = await holdFileOpen(target);
		try {
			const { jobId } = await running.api.json('POST', '/shares/transfer', {
				json: {
					op: 'move',
					from: { shareId: share.id, paths: [name] },
					to: { shareId: share.id, path: 'in-use' },
					onConflict: 'overwrite'
				}
			});
			const job = await running.api.waitJob(jobId);
			assert.equal(job.state, 'failed', JSON.stringify(job));
			assert.equal(job.error, 'entry_in_use');
		} finally {
			await held.release();
		}
		assert.equal(await fs.readFile(target, 'utf8'), 'old', '置き先は元のまま');
		assert.equal(await fs.readFile(path.join(folder, name), 'utf8'), 'new', '移動の元も残る');
	});
});
