import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import { cleanUp, entryNames, installFresh, launch, quit, removePackage } from '../lib/windows.mjs';
import { runPowerShell } from '../lib/powershell.mjs';

// NAS (SMB の共有) の中のフォルダーを共有にしたとき (docs/folders.md「NAS と外付けのディスク」)。割り当てたドライブから
// 選んでも UNC のパスで登録すること (src/api/roots.rs)、ごみ箱を使えない場所の判定 (src/api/shares/fsops.rs)。
// 手元の NAS を環境変数 WEBLAV_E2E_SMB (`\\<ホスト>\<共有>`) で指したときだけ回す。共有の直下に確かめ用のフォルダーを
// 作って消し、空いているドライブの文字に一時的に割り当てる

const SMB = process.env.WEBLAV_E2E_SMB;

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

	test.before(async () => {
		// 前からあるものを後片付けで消さないよう、作れた新しいフォルダーの名前だけを持つ
		folderName = path.basename(await fs.mkdtemp(path.join(SMB, 'weblav-e2e-')));
		await fs.writeFile(path.join(SMB, folderName, 'hello'), 'hello');
		drive = `${await freeDriveLetter()}:`;
		await runPowerShell('net use $args[0] $args[1] /persistent:no | Out-Null', [drive, SMB]);
		running = await launch(await installFresh());
		await running.api.createAdmin();
	});

	test.after(async () => {
		await cleanUp([
			() => quit(running),
			() => removePackage(),
			() => drive && runPowerShell('net use $args[0] /delete /y | Out-Null', [drive]),
			() => folderName && fs.rm(path.join(SMB, folderName), { recursive: true, force: true })
		]);
	});

	test('割り当てたドライブのフォルダーを登録すると UNC のパスになり、中を見られる。ごみ箱は使えないと伝える', async () => {
		const share = await running.api.addRoot(`${drive}\\${folderName}`);
		assert.equal(share.path.toLowerCase(), path.join(SMB, folderName).toLowerCase());
		assert.deepEqual(entryNames(await running.api.browse(share.id)), ['hello']);
		const check = await running.api.json('POST', `/shares/${share.id}/delete/check`, {
			json: { paths: ['hello'] }
		});
		assert.equal(check.trash, false, '消すと元に戻せない');
	});
});
