// `just msix-check` の入口。MSIX を作り、MSIX 版の確認 (`msix/*.test.mjs`) を1ファイルずつ順に回す。
//
// `just msix` で配る形の MSIX (dist/weblav-v<版>.msix。WACK で見る) を作り、同じ exe から確かめ用のパッケージを
// 2つの版で作る (scripts/msix.mjs の `--e2e`)。`--no-build` を付けると、作らずに前に作ったもので回す。
// 引数でテストファイルを選べる。`tray` のように名前だけなら `msix/tray.test.mjs`。
//
// 回している間は常用の WebLAV を止め、終わったら (動いていたなら) 起動し直す。トレイのアイコンの名前が同じで、
// 確かめ用のものと見分けられないため。
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.platform !== 'win32') {
	console.error('[msix-check] Windows で回してください');
	process.exit(1);
}

const e2eDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(e2eDir, '..');
const {
	DAILY_PACKAGE_NAME,
	NEW_REVISION,
	OLD_REVISION,
	openApp,
	packagePids,
	readPackage,
	removePackage,
	stopAll
} = await import('./lib/windows.mjs');
const { runPowerShell, stopPowerShell } = await import('./lib/powershell.mjs');

const args = process.argv.slice(2);
const build = !args.includes('--no-build');
const names = args.filter((arg) => !arg.startsWith('--'));
const msixDir = path.join(e2eDir, 'msix');
const files = (
	names.length > 0
		? names.map((name) => (name.endsWith('.mjs') ? name : `${name}.test.mjs`))
		: fs.readdirSync(msixDir).filter((name) => name.endsWith('.test.mjs'))
)
	.sort()
	.map((name) => path.join(msixDir, name));

// 子の終わりを待って後片付けをするため、Ctrl+C では終わらない (同じコンソールのテストのプロセスにも届き、そちらが止まる)
process.on('SIGINT', () => {});

function run(command, commandArgs, options = {}) {
	return new Promise((resolve, reject) => {
		const child = spawn(command, commandArgs, { cwd: repoRoot, stdio: 'inherit', ...options });
		child.once('error', reject);
		child.once('exit', (code) => resolve(code ?? 1));
	});
}

/** ロックの間は前面が LogonUI になり、窓やメニューが前に出ないので、始める前に落とす */
async function assertUnlocked() {
	const locked = (
		await runPowerShell(
			"if (Get-Process -Name LogonUI -ErrorAction SilentlyContinue) { 'locked' } else { 'unlocked' }"
		)
	).trim();
	if (locked === 'locked')
		throw new Error('画面がロックされています。ロックを解いてから回してください');
}

/** 回している間に画面が消えると、トレイのメニューが出ずに落ちる。終わるまで画面を点けたままにする */
function keepDisplayOn() {
	const child = spawn(
		'powershell',
		[
			'-NoProfile',
			'-Command',
			`Add-Type -Namespace E2E -Name Power -MemberDefinition '[DllImport("kernel32.dll")] public static extern uint SetThreadExecutionState(uint flags);'
# ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
[void][E2E.Power]::SetThreadExecutionState([uint32]2147483651)
[void][Console]::In.ReadLine()`
		],
		{ stdio: ['pipe', 'ignore', 'inherit'], windowsHide: true }
	);
	return () => child.stdin.end('\n');
}

let code = 1;
let daily = null;
let dailyWasRunning = false;
const release = keepDisplayOn();
try {
	await assertUnlocked();
	if (build) {
		code = await run('just', ['msix']);
		for (const revision of [OLD_REVISION, NEW_REVISION]) {
			if (code === 0)
				code = await run('node', ['scripts/msix.mjs', '--e2e', `--revision=${revision}`]);
		}
		if (code !== 0) throw new Error('MSIX を作れませんでした');
	}
	daily = await readPackage(DAILY_PACKAGE_NAME);
	dailyWasRunning = daily !== null && (await packagePids(daily)).length > 0;
	if (dailyWasRunning) {
		console.log('[msix-check] 常用の WebLAV を止めます (終わったら起動し直します)');
		await stopAll(DAILY_PACKAGE_NAME);
	}
	// 確かめ用の WebLAV は同じパッケージを入れ替えるので、ファイルを並べて回さない
	code = await run(process.execPath, ['--test', '--test-concurrency=1', ...files], { cwd: e2eDir });
} catch (error) {
	code = 1;
	console.error(`[msix-check] ${error.message}`);
} finally {
	try {
		await stopAll();
		await removePackage();
	} catch (error) {
		console.error(`[msix-check] 確かめ用のパッケージを外せませんでした: ${error.message}`);
	}
	if (dailyWasRunning) {
		try {
			await openApp(daily);
		} catch (error) {
			console.error(`[msix-check] 常用の WebLAV を起動し直せませんでした: ${error.message}`);
		}
	}
	release();
	await stopPowerShell();
}
process.exit(code);
