import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import {
	blockSmb,
	cleanUp,
	connectSmb,
	disconnectSmb,
	entryNames,
	installFresh,
	launch,
	quit,
	removePackage,
	waitFor
} from '../lib/windows.mjs';
import { runPowerShell } from '../lib/powershell.mjs';

// NAS (SMB の共有) の中のフォルダーを共有にしたとき (docs/folders.md「NAS と外付けのディスク」)。割り当てたドライブから
// 選んでも UNC のパスで登録すること (src/api/roots.rs)、ごみ箱を使えない場所の判定 (src/api/shares/fsops.rs)。
// 手元の NAS を環境変数 WEBLAV_E2E_SMB (`\\<ホスト>\<共有>`) で指したときだけ回す。共有の直下に確かめ用のフォルダーを
// 作って消し、空いているドライブの文字に一時的に割り当てる。NAS が戻ったときの確認では、この PC から NAS への SMB を
// ファイアウォールで一時的に止める (UAC の確かめの窓が出る。止めている間は、この PC からその NAS に届かない)

const SMB = process.env.WEBLAV_E2E_SMB;

/**
 * 確かめ用のファイル。中を読んだ直後 (閉じてから十数秒のうち) に SMB を止めると、NAS の側にそのファイルを開いたままの
 * 古い接続が残り、消しても消し待ちのまま確かめ用フォルダーを消せなくなる (Nano と LinkStation の SMB 1 で、WebLAV を
 * 使わずに読んでも起きた)。WebLAV の一覧は、拡張子で種類の決まらないファイルの先頭を読んでテキストかを見るので
 * (src/api/text_files.rs の `is_text_file`)、種類が拡張子で決まり、OS の縮小画像 (src/os_thumbnail) の対象でもない拡張子にする
 */
const FILE_NAME = 'hello.mp3';

/** 前の回が残した確かめ用フォルダーを消す。消し待ちのファイルが残っていれば、警告に留めて次の回に回す */
async function removeLeftover(dir) {
	try {
		await fs.rm(dir, { recursive: true, force: true });
	} catch (error) {
		if (!['EPERM', 'EBUSY', 'ENOTEMPTY', 'EACCES'].includes(error.code)) throw error;
		console.warn(`[msix-check] 前の回の ${dir} を、消し待ちのファイルがあって消せません`);
	}
}

/** 使われていないドライブの文字 (Z から逆に探す) */
async function freeDriveLetter() {
	const stdout = await runPowerShell(`
$used = @(Get-PSDrive -PSProvider FileSystem | ForEach-Object { $_.Name }) + @(Get-SmbMapping -ErrorAction SilentlyContinue | ForEach-Object { $_.LocalPath.TrimEnd(':') })
foreach ($c in [char[]]'ZYXWVUTSRQPONMLKJIHG') { if ($used -notcontains [string]$c) { return [string]$c } }
return ''
`);
	const letter = stdout.trim();
	if (!letter) throw new Error('空いているドライブの文字がありません');
	return letter;
}

test.describe('MSIX 版: NAS の共有', { skip: !SMB && 'WEBLAV_E2E_SMB が無い' }, () => {
	let running;
	let folderName;
	let drive;
	let share;
	let block;

	test.before(async () => {
		await connectSmb(SMB);
		// 前の回が途中で止まって残した確かめ用フォルダー (mkdtemp の名前の形のものだけ) を消す
		for (const name of await fs.readdir(SMB)) {
			if (/^weblav-e2e-[A-Za-z0-9]{6}$/.test(name)) await removeLeftover(path.join(SMB, name));
		}
		// 前からあるものを後片付けで消さないよう、作れた新しいフォルダーの名前だけを持つ
		folderName = path.basename(await fs.mkdtemp(path.join(SMB, 'weblav-e2e-')));
		await fs.writeFile(path.join(SMB, folderName, FILE_NAME), 'hello');
		drive = `${await freeDriveLetter()}:`;
		await runPowerShell('net use $args[0] $args[1] /persistent:no | Out-Null', [drive, SMB]);
		running = await launch(await installFresh());
		await running.api.createAdmin();
	});

	test.after(async () => {
		await cleanUp([
			() => block?.release(),
			() => quit(running),
			() => removePackage(),
			() => drive && runPowerShell('net use $args[0] /delete /y | Out-Null', [drive]),
			() => folderName && fs.rm(path.join(SMB, folderName), { recursive: true, force: true }),
			() => disconnectSmb(SMB)
		]);
	});

	test('割り当てたドライブのフォルダーを登録すると UNC のパスになり、中を見られる。ごみ箱は使えないと伝える', async () => {
		share = await running.api.addRoot(`${drive}\\${folderName}`);
		assert.equal(share.path.toLowerCase(), path.join(SMB, folderName).toLowerCase());
		assert.deepEqual(entryNames(await running.api.browse(share.id)), [FILE_NAME]);
		const check = await running.api.json('POST', `/shares/${share.id}/delete/check`, {
			json: { paths: [FILE_NAME] }
		});
		assert.equal(check.trash, false, '消すと元に戻せない');
		// 登録は UNC のパスで、この先は割り当てを使わないので、ここで外す
		await runPowerShell('net use $args[0] /delete /y | Out-Null', [drive]);
		drive = null;
	});

	test('NAS に届かなくなると「つながっていません」になり、届くようになると操作しなくてもまた見られる', async () => {
		assert.ok(share, '前のテストで共有にしている');
		block = await blockSmb(SMB);
		await waitFor(
			() => running.api.request('GET', `/shares/${share.id}/browse?path=`).then((r) => r.status),
			(status) => status === 503,
			{ label: 'NAS の共有がつながっていないと返る', timeout: 30_000, interval: 1000 }
		);
		const roots = await running.api.json('GET', '/admin/roots');
		assert.equal(roots.find((root) => root.id === share.id).status, 'unreachable');

		await block.release();
		block = null;
		const browse = await waitFor(() => running.api.browse(share.id).catch(() => null), Boolean, {
			label: '届くようになると、また見られる',
			timeout: 60_000,
			interval: 1000
		});
		assert.deepEqual(entryNames(browse), [FILE_NAME]);
	});
});
