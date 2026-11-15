import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import {
	NEW_REVISION,
	OLD_REVISION,
	STARTUP_TASK_STATE,
	UNREDIRECTED_DIRS,
	addPackage,
	appDirs,
	cleanUp,
	entryNames,
	executable,
	firewallRules,
	launch,
	msixPath,
	packageDataDir,
	quit,
	readPackage,
	readStartupTaskState,
	relaunch,
	removePackage,
	sleep,
	stopAll,
	visibleWindows,
	workDir,
	writeStartupTaskState
} from '../lib/windows.mjs';

// パッケージとしての出し入れ (docs/distribution.md「MSIX (Windows)」): 更新でデータとログイン時の起動の状態が残ること、
// データの置き場がパッケージごとの場所に回されること、マニフェストのファイアウォールの規則、アンインストールで消えること

const exists = (file) =>
	fs.access(file).then(
		() => true,
		() => false
	);

test.describe('MSIX 版: パッケージ', () => {
	let running;
	let folder;
	let share;

	test.before(async () => {
		await stopAll();
		await removePackage();
		folder = await workDir('package');
		await fs.writeFile(path.join(folder, 'hello'), 'hello');
	});

	test.after(async () => {
		await cleanUp([
			() => quit(running),
			() => removePackage(),
			() => folder && fs.rm(folder, { recursive: true, force: true })
		]);
	});

	test('前の版から更新すると、データ・登録したフォルダー・サインイン時に起動の状態がそのまま残る', async () => {
		const old = await addPackage(msixPath(OLD_REVISION));
		running = await launch(old);
		await running.api.createAdmin();
		share = await running.api.addRoot(folder);
		// 利用者がメニューで切った状態 (Disabled) にしておき、更新で既定 (有効) に戻らないことを見る
		await writeStartupTaskState(old, STARTUP_TASK_STATE.disabled);

		const updated = await addPackage(msixPath(NEW_REVISION));
		assert.equal(updated.familyName, old.familyName);
		assert.notEqual(updated.version, old.version, '新しい版に替わっている');
		assert.equal(await readStartupTaskState(updated), STARTUP_TASK_STATE.disabled);
		running = await relaunch({ ...running, pkg: updated });
		assert.deepEqual(entryNames(await running.api.browse(share.id)), ['hello']);
	});

	test('データはパッケージごとの場所に回され、止めて起動し直しても残る', async () => {
		const pkg = await readPackage();
		assert.ok(await exists(path.join(appDirs(pkg).data, 'weblav.db')), 'DB が回された先にある');
		for (const dir of UNREDIRECTED_DIRS)
			assert.equal(await exists(dir), false, `${dir} には何もできない`);
		await quit(running);
		running = await relaunch({ ...running, pkg });
		assert.deepEqual(entryNames(await running.api.browse(share.id)), ['hello']);
	});

	test('ファイアウォールの受信の規則がプライベートとドメインだけにあり、待ち受けを始めても許可を尋ねる窓が出ない', async () => {
		const pkg = await readPackage();
		const before = new Set((await visibleWindows()).map((w) => w.hwnd));
		await quit(running);
		running = await relaunch({ ...running, pkg });
		// 尋ねる窓は、待ち受けを始めてすぐに出る
		await sleep(3000);
		const appeared = (await visibleWindows()).filter(
			(w) => !before.has(w.hwnd) && /セキュリティ|Security/.test(w.title)
		);
		assert.deepEqual(appeared, [], '許可を尋ねる窓が出ていない');

		const inbound = (await firewallRules(executable(pkg))).filter(
			(rule) => rule.direction === 'Inbound' && rule.action === 'Allow'
		);
		assert.ok(inbound.length > 0, '受信を許す規則がある');
		for (const rule of inbound) {
			assert.equal(rule.enabled, 'True');
			assert.ok(
				!rule.profile.includes('Public'),
				`パブリックのネットワークを許していない: ${rule.profile}`
			);
		}
		assert.ok(
			inbound.some((rule) => rule.profile.includes('Private')),
			'プライベートのネットワークで受ける'
		);
	});

	test('アンインストールで、設定とデータ (パッケージごとの場所) が消える', async () => {
		const pkg = await readPackage();
		await quit(running);
		running = null;
		const dir = packageDataDir(pkg);
		assert.ok(await exists(dir), `外す前に ${dir} がある`);
		await removePackage();
		assert.equal(await readPackage(), null);
		assert.equal(await exists(dir), false, `${dir} が消えている`);
	});
});
