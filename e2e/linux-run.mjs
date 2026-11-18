// Linux の自動の確認 (`linux/*.test.mjs`) を1ファイルずつ順に回す。VM の中で linux-vm-run.mjs が動かす
// (手元のデスクトップでは回さない。ログイン時に起動の .desktop と、ブラウザーの既定を書き換えるため)。
//
// 引数でテストファイルを選べる。`tray` のように名前だけなら `linux/tray.test.mjs`
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.platform !== 'linux') {
	console.error('[linux-check] Linux で回してください');
	process.exit(1);
}

const e2eDir = path.dirname(fileURLToPath(import.meta.url));
const { HOME, preflight } = await import('./lib/linux.mjs');
try {
	await preflight();
} catch (error) {
	console.error(`[linux-check] ${error.message}`);
	process.exit(1);
}

const linuxDir = path.join(e2eDir, 'linux');
const names = process.argv.slice(2);
const files = (
	names.length > 0
		? names.map((name) => (name.endsWith('.mjs') ? name : `${name}.test.mjs`))
		: fs.readdirSync(linuxDir).filter((name) => name.endsWith('.test.mjs'))
)
	.sort()
	.map((name) => path.join(linuxDir, name));

// 確かめ用の WebLAV は同じデータの置き場を使うので、ファイルを並べて回さない
const result = spawnSync(process.execPath, ['--test', '--test-concurrency=1', ...files], {
	cwd: e2eDir,
	stdio: 'inherit'
});
fs.rmSync(HOME, { recursive: true, force: true });
process.exit(result.status ?? 1);
