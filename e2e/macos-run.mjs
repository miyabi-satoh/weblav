// `just macos-check` の入口。macOS の自動の確認 (`macos/*.test.mjs`) を1ファイルずつ順に回す。
//
// ビルドはしない。/Applications/WebLAV.app で回す (別のものは環境変数 WEBLAV_APP で指す)。
// 引数でテストファイルを選べる。`tray` のように名前だけなら `macos/tray.test.mjs`
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.platform !== 'darwin') {
	console.error('[macos-check] macOS で回してください');
	process.exit(1);
}

const e2eDir = path.dirname(fileURLToPath(import.meta.url));
const { APP, HOME, assertGuiSession, assertPermissions } = await import('./lib/macos.mjs');
try {
	await assertGuiSession();
	await assertPermissions();
	if (!fs.existsSync(APP))
		throw new Error(`${APP} がありません。先に just install-mac で入れてください`);
} catch (error) {
	console.error(`[macos-check] ${error.message}`);
	process.exit(1);
}

const macosDir = path.join(e2eDir, 'macos');
const names = process.argv.slice(2);
const files = (
	names.length > 0
		? names.map((name) => (name.endsWith('.mjs') ? name : `${name}.test.mjs`))
		: fs.readdirSync(macosDir).filter((name) => name.endsWith('.test.mjs'))
)
	.sort()
	.map((name) => path.join(macosDir, name));

// 確かめ用の WebLAV は同じデータの置き場を使うので、ファイルを並べて回さない
const result = spawnSync(process.execPath, ['--test', '--test-concurrency=1', ...files], {
	cwd: e2eDir,
	stdio: 'inherit'
});
fs.rmSync(HOME, { recursive: true, force: true });
process.exit(result.status ?? 1);
