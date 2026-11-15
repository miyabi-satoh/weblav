// MSIX 版の確認 (`just msix-check`) の土台。確かめ用のパッケージ (amiiby.WebLAV.E2E) を入れ、トレイ・ログイン時の起動・
// ファイアウォール・フォルダー選択の窓・ごみ箱を、配る形のまま動かして OS の側で読む (→ e2e/README.md「MSIX 版の確認」)。
//
// 確かめ用のパッケージは、常用の版 (amiiby.WebLAV) と名前と表示名だけが違う (scripts/msix.mjs の `--e2e`)。
// データ・ログイン時の起動・ファイアウォールの規則はパッケージごとに分かれるので、常用の版のものには触れない。
// トレイのアイコンの名前 (ツールチップ) は同じなので、回している間は常用の版を止める (msix-run.mjs)。
import { spawn } from 'node:child_process';
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Api, waitFor } from './common.mjs';
import { runPowerShell } from './powershell.mjs';

export { Api, PASSWORD, assertMissing, cleanUp, entryNames, sleep, waitFor } from './common.mjs';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');

/** scripts/msix.mjs の E2E_NAME と揃える */
export const PACKAGE_NAME = 'amiiby.WebLAV.E2E';
/** 常用の版。回している間は止め、終わったら起動し直す */
export const DAILY_PACKAGE_NAME = 'amiiby.WebLAV';
/** installer/msix/AppxManifest.xml の Application の Id と StartupTask の TaskId */
const APPLICATION_ID = 'WebLAV';
const STARTUP_TASK_ID = 'WebLAV';
/** 常用のもの (既定 8370) とぶつからない番号 */
export const PORT = 18370;

/** `node scripts/msix.mjs --e2e --revision=<n>` が作る MSIX。更新を確かめるため、版の4つ目だけが違うものを2つ作る */
export function msixPath(revision) {
	return path.join(repoRoot, 'target', 'msix-e2e', `weblav-e2e-${revision}.msix`);
}
export const OLD_REVISION = 1;
export const NEW_REVISION = 2;

/**
 * 入っているパッケージ。入っていなければ null
 *
 * @returns {Promise<{ familyName: string, installLocation: string, version: string } | null>}
 */
export async function readPackage(name = PACKAGE_NAME) {
	const stdout = await runPowerShell(
		`
$p = Get-AppxPackage -Name $args[0] | Select-Object -First 1
if ($null -eq $p) { return 'null' }
ConvertTo-Json -Compress -InputObject @{ familyName = $p.PackageFamilyName; installLocation = $p.InstallLocation; version = [string]$p.Version }
`,
		[name]
	);
	return JSON.parse(stdout.trim());
}

/** MSIX を入れる。入っている版より新しければ、データを残したまま更新する (動いていれば止める) */
export async function addPackage(msix) {
	await runPowerShell('Add-AppxPackage -Path $args[0] -ForceApplicationShutdown', [msix]);
	return readPackage();
}

/** 確かめ用のパッケージを外す。入っていなければ何もしない */
export async function removePackage() {
	await runPowerShell('Get-AppxPackage -Name $args[0] | Remove-AppxPackage', [PACKAGE_NAME]);
}

/** 前の回が残したものを外してから、今の版を入れる */
export async function installFresh() {
	await stopAll();
	await removePackage();
	return addPackage(msixPath(NEW_REVISION));
}

/** パッケージごとの場所 (`%LOCALAPPDATA%\Packages\<ファミリー名>`)。外すとまるごと消える */
export function packageDataDir(pkg) {
	return path.join(process.env.LOCALAPPDATA, 'Packages', pkg.familyName);
}

/**
 * 設定とデータの置き場。`%APPDATA%`・`%LOCALAPPDATA%` に書いたものは、パッケージごとの場所の LocalCache に回される
 * (docs/distribution.md「MSIX (Windows)」の「設定とデータの置き場所」。`ProjectDirs` の amiiby\weblav の下)
 */
export function appDirs(pkg) {
	const cache = path.join(packageDataDir(pkg), 'LocalCache');
	return {
		config: path.join(cache, 'Roaming', 'amiiby', 'weblav', 'config'),
		data: path.join(cache, 'Local', 'amiiby', 'weblav', 'data')
	};
}

/** 回されなかったときに書かれる場所。パッケージの中からは、ここに何もできないはず */
export const UNREDIRECTED_DIRS = [
	path.join(process.env.APPDATA ?? '', 'amiiby', 'weblav'),
	path.join(process.env.LOCALAPPDATA ?? '', 'amiiby', 'weblav')
];

/** パッケージの中の weblav.exe */
export function executable(pkg) {
	return path.join(pkg.installLocation, 'weblav.exe');
}

/** 動いている weblav.exe のうち、`pkg` の中のもののプロセス番号 */
export async function packagePids(pkg) {
	const stdout = await runPowerShell(
		`$exe = $args[0]
ConvertTo-Json -Compress -InputObject @(Get-Process -Name weblav -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe } | ForEach-Object { $_.Id })`,
		[executable(pkg)]
	);
	return JSON.parse(stdout.trim() || '[]');
}

/** スタートメニューから開くのと同じ起動 */
export async function openApp(pkg) {
	await runPowerShell('Start-Process ("shell:AppsFolder\\" + $args[0])', [
		`${pkg.familyName}!${APPLICATION_ID}`
	]);
}

/**
 * 確かめ用の WebLAV を起動し、応答できるまで待つ。`fresh` ならデータを消してから始める。
 * 返すのは、動いているもの (パッケージ・プロセス番号・実際のポート・API)
 */
export async function launch(pkg, { fresh = true, port = PORT } = {}) {
	await stopAll();
	const dirs = appDirs(pkg);
	if (fresh) await fs.rm(dirs.data, { recursive: true, force: true });
	// LocalCache の Local と Roaming は、ふつうは初めて起動したときに OS が作る。入れた直後に外から Roaming の下だけを
	// 作ると、Local が作られず、データが回されずに実際の %LOCALAPPDATA% に書かれた (Windows 11 の実機で確かめた)。両方を作っておく
	await fs.mkdir(path.dirname(path.dirname(path.dirname(dirs.data))), { recursive: true });
	await fs.mkdir(dirs.config, { recursive: true });
	await fs.writeFile(path.join(dirs.config, 'config.toml'), `[server]\nport = ${port}\n`);
	await fs.rm(path.join(dirs.data, 'weblav.addr'), { force: true });
	await openApp(pkg);
	const addr = await waitFor(
		async () => {
			for (const dir of UNREDIRECTED_DIRS)
				if (await fs.stat(dir).catch(() => null))
					throw new Error(`データがパッケージごとの場所に回されず、${dir} に書かれた`);
			return fs.readFile(path.join(dirs.data, 'weblav.addr'), 'utf8').catch(() => null);
		},
		(text) => text !== null && text.includes(':'),
		{ label: 'WebLAV が起動してアドレスを書く', timeout: 30_000 }
	);
	const actualPort = Number(addr.trim().split(':').pop());
	const [pid] = await waitFor(
		() => packagePids(pkg),
		(pids) => pids.length === 1,
		{ label: '確かめ用の weblav.exe が1つ動く' }
	);
	const api = new Api(actualPort);
	await waitFor(
		() => api.status('GET', '/setup/status').catch(() => 0),
		(code) => code > 0,
		{ label: 'API が応える' }
	);
	return { pkg, pid, port: actualPort, api };
}

/** データを残したまま起動し直し、ログインした状態にする */
export async function relaunch(running) {
	const next = await launch(running.pkg, { fresh: false });
	await next.api.login();
	return next;
}

/** 確かめ用の weblav.exe を全部止め、終わるまで待つ */
export async function stopAll(name = PACKAGE_NAME) {
	const pkg = await readPackage(name);
	if (!pkg) return;
	await stopPids(await packagePids(pkg), pkg);
}

async function stopPids(pids, pkg) {
	if (pids.length === 0) return;
	await runPowerShell(
		'foreach ($id in $args) { Stop-Process -Id ([int]$id) -Force -ErrorAction SilentlyContinue }',
		pids.map(String)
	);
	await waitFor(
		() => packagePids(pkg),
		(left) => left.every((pid) => !pids.includes(pid)),
		{ label: 'weblav.exe が終わる' }
	);
}

/** 確かめ用の WebLAV を止める */
export async function quit(running) {
	if (running) await stopPids([running.pid], running.pkg);
}

/** ログのファイルをつないだもの (起動のたびに増えるので、数えて比べる) */
export async function readLog(pkg) {
	const dir = path.join(appDirs(pkg).data, 'logs');
	try {
		const names = await fs.readdir(dir);
		const texts = await Promise.all(names.map((name) => fs.readFile(path.join(dir, name), 'utf8')));
		return texts.join('');
	} catch {
		return '';
	}
}

// StartupTask の状態は、パッケージのデータの下のレジストリに置かれる。
// 値は Windows.ApplicationModel.StartupTaskState と同じ (0: Disabled、1: DisabledByUser、2: Enabled)
function startupTaskKey(pkg) {
	return `HKCU:\\Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\SystemAppData\\${pkg.familyName}\\${STARTUP_TASK_ID}`;
}

export const STARTUP_TASK_STATE = Object.freeze({ disabled: 0, disabledByUser: 1, enabled: 2 });

/** StartupTask の状態。まだ一度も読み書きされていなければ null */
export async function readStartupTaskState(pkg) {
	const stdout = await runPowerShell(
		`
$item = Get-ItemProperty -LiteralPath $args[0] -ErrorAction SilentlyContinue
if ($null -eq $item -or $null -eq $item.State) { return 'null' }
[string]$item.State
`,
		[startupTaskKey(pkg)]
	);
	return JSON.parse(stdout.trim());
}

/**
 * StartupTask の状態を書き換える。「設定」のスタートアップ アプリで止めた状態 (DisabledByUser) を作るのに使う
 * (State を 1 にすると DisabledByUser として読まれる)
 */
export async function writeStartupTaskState(pkg, state) {
	await runPowerShell(
		`
if (-not (Test-Path -LiteralPath $args[0])) { New-Item -Path $args[0] -Force | Out-Null }
Set-ItemProperty -LiteralPath $args[0] -Name State -Value ([int]$args[1]) -Type DWord
`,
		[startupTaskKey(pkg), String(state)]
	);
}

/** OS の表示言語 (`GetUserDefaultUILanguage`。トレイが文言を決めるのと同じ。→ src/tray/mod.rs の tray_locale) */
export async function uiLanguageId() {
	const stdout = await runPowerShell(`
Add-Type @"
using System.Runtime.InteropServices;
public static class E2ELanguage {
    [DllImport("kernel32.dll")] public static extern ushort GetUserDefaultUILanguage();
}
"@
[string][E2ELanguage]::GetUserDefaultUILanguage()
`);
	return Number(stdout.trim());
}

/** `exe` を指すファイアウォールの規則 (向き・許可か・プロファイル) */
export async function firewallRules(exe) {
	const stdout = await runPowerShell(
		`
$rules = @(Get-NetFirewallApplicationFilter -Program $args[0] -ErrorAction SilentlyContinue | Get-NetFirewallRule | ForEach-Object {
    @{ name = $_.Name; direction = [string]$_.Direction; action = [string]$_.Action; profile = [string]$_.Profile; enabled = [string]$_.Enabled }
})
ConvertTo-Json -Compress -InputObject $rules
`,
		[exe]
	);
	return JSON.parse(stdout.trim() || '[]');
}

const WINDOW_TYPE = `
Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class E2EWindows {
    delegate bool EnumProc(IntPtr hWnd, IntPtr param);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback, IntPtr param);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassNameW(IntPtr hWnd, StringBuilder buffer, int max);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowTextW(IntPtr hWnd, StringBuilder buffer, int max);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(POINT point);
    [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr hWnd, uint flags);
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll")] static extern bool SetProcessDPIAware();

    static string Text(IntPtr hWnd, bool cls) {
        var buffer = new StringBuilder(256);
        if (cls) GetClassNameW(hWnd, buffer, 256); else GetWindowTextW(hWnd, buffer, 256);
        return buffer.ToString();
    }

    // 見えているトップレベルの窓。1行に「ハンドル\\tプロセス番号\\tクラス\\t題」
    public static List<string> Visible() {
        var found = new List<string>();
        EnumWindows((hWnd, param) => {
            if (IsWindowVisible(hWnd)) {
                uint pid;
                GetWindowThreadProcessId(hWnd, out pid);
                found.Add(hWnd.ToInt64() + "\\t" + pid + "\\t" + Text(hWnd, true) + "\\t" + Text(hWnd, false));
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    // 窓の真ん中に見えているのが、その窓 (か、その中の窓) か。ほかの窓の後ろに隠れていれば偽
    public static bool OnTop(IntPtr hWnd) {
        if (!SetProcessDpiAwarenessContext(new IntPtr(-4))) SetProcessDPIAware();
        RECT r;
        if (!GetWindowRect(hWnd, out r)) return false;
        var center = new POINT { X = (r.Left + r.Right) / 2, Y = (r.Top + r.Bottom) / 2 };
        // GA_ROOT = 2
        return GetAncestor(WindowFromPoint(center), 2) == hWnd;
    }
}
"@
`;

/** 見えているトップレベルの窓の一覧 */
export async function visibleWindows() {
	const stdout = await runPowerShell(
		`${WINDOW_TYPE}\nConvertTo-Json -Compress -InputObject @([E2EWindows]::Visible())`
	);
	return JSON.parse(stdout.trim() || '[]').map((line) => {
		const [hwnd, pid, className, title] = line.split('\t');
		return { hwnd, pid: Number(pid), className, title };
	});
}

/** 窓の真ん中に、その窓が見えているか (ほかの窓より前に出ているか) */
export async function isOnTop(hwnd) {
	const stdout = await runPowerShell(
		`${WINDOW_TYPE}\n[E2EWindows]::OnTop([IntPtr][long]$args[0])`,
		[hwnd]
	);
	return stdout.trim() === 'True';
}

/** 窓に WM_CLOSE を送る */
export async function closeWindow(hwnd) {
	await runPowerShell(
		`${WINDOW_TYPE}\n[void][E2EWindows]::PostMessageW([IntPtr][long]$args[0], 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)`,
		[hwnd]
	);
}

/** `pid` のフォルダー選択の窓 (コモン ダイアログのクラス `#32770`)。出ていなければ null */
export async function folderDialog(pid) {
	const windows = await visibleWindows();
	return windows.find((w) => w.pid === pid && w.className === '#32770') ?? null;
}

/**
 * フォルダー選択の窓で、場所の欄に `folder` を入れて「フォルダーの選択」を押す。キーは打たず、窓のメッセージで
 * 欄に値を入れてボタンを押す (欄は 1152、ボタンは IDOK の 1。コモン ダイアログの決まった番号)。
 * 窓が出た直後は欄がまだ無いので、できるまで待つ
 */
export async function chooseInDialog(hwnd, folder) {
	await waitFor(
		async () =>
			(
				await runPowerShell(
					`
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class E2EDialog {
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr dialog, int id);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessageW(IntPtr hWnd, uint msg, IntPtr wParam, string lParam);
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
}
"@
$dialog = [IntPtr][long]$args[0]
$edit = [E2EDialog]::GetDlgItem($dialog, 1152)
$ok = [E2EDialog]::GetDlgItem($dialog, 1)
if ($edit -eq [IntPtr]::Zero -or $ok -eq [IntPtr]::Zero) { return 'not-ready' }
# WM_SETTEXT = 0x000C、BM_CLICK = 0x00F5。押すのは PostMessage にして、窓が閉じるまで待たない
[void][E2EDialog]::SendMessageW($edit, 0x000C, [IntPtr]::Zero, $args[1])
[void][E2EDialog]::PostMessageW($ok, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero)
return 'pressed'
`,
					[hwnd, folder]
				)
			).trim(),
		(result) => result === 'pressed',
		{ label: 'フォルダー選択の窓の場所の欄ができる' }
	);
}

/** 出ているフォルダー選択の窓を閉じる (取り消し)。出ていなければ何もしない */
export async function cancelDialog(pid) {
	const dialog = await folderDialog(pid);
	if (!dialog) return;
	await closeWindow(dialog.hwnd);
	await waitFor(
		() => folderDialog(pid),
		(found) => found === null,
		{ label: 'フォルダー選択の窓が閉じる' }
	);
}

/**
 * フォルダー選択の窓で `folder` を選んで共有に足す。返すのは足した共有。
 * 窓を出させる要求は窓が閉じるまで返らないので、待つ間に窓を操作する
 */
export async function pickAndAddRoot(running, folder) {
	const picked = running.api.pick();
	const dialog = await waitFor(() => folderDialog(running.pid), Boolean, {
		label: 'フォルダー選択の窓が出る'
	});
	await chooseInDialog(dialog.hwnd, folder);
	const chosen = await picked;
	return running.api.addRoot(chosen);
}

/** ごみ箱の中の名前 (表示の名前。拡張子を隠す設定だと拡張子が付かないので、確かめ用のファイルには拡張子を付けない) */
export async function recycleBinNames() {
	const stdout = await runPowerShell(`
$bin = (New-Object -ComObject Shell.Application).NameSpace(10)
ConvertTo-Json -Compress -InputObject @($bin.Items() | ForEach-Object { $_.Name })
`);
	return JSON.parse(stdout.trim() || '[]');
}

/**
 * ほかのアプリが開いているのと同じに、ファイルを共有なしで開いたままにする。返ってきた `release` で放す。
 * 別の PowerShell のプロセスで開く (立ち上げたままの PowerShell は呼ぶたびにスコープが替わり、開いたままにできない)
 */
export async function holdFileOpen(file) {
	const child = spawn(
		'powershell',
		[
			'-NoProfile',
			'-Command',
			"$f = [IO.File]::Open($env:WEBLAV_E2E_HOLD, 'Open', 'ReadWrite', 'None'); [Console]::Out.WriteLine('held'); [void][Console]::In.ReadLine(); $f.Close()"
		],
		{
			stdio: ['pipe', 'pipe', 'inherit'],
			windowsHide: true,
			env: { ...process.env, WEBLAV_E2E_HOLD: file }
		}
	);
	await new Promise((resolve, reject) => {
		child.stdout.once('data', resolve);
		child.once('error', reject);
		child.once('exit', (code) =>
			reject(new Error(`ファイルを開いたままにできませんでした (code=${code})`))
		);
	});
	return {
		release: () =>
			new Promise((resolve) => {
				child.once('exit', resolve);
				child.stdin.end('\n');
			})
	};
}

/** 待ち受けているだけのサーバーで、ポートを先に取る */
export function occupyPort(port) {
	return new Promise((resolve, reject) => {
		const child = spawn(process.execPath, [
			'-e',
			`require('node:net').createServer().listen(${port}, '0.0.0.0', () => console.log('ready'))`
		]);
		child.stdout.once('data', () => resolve(child));
		child.once('error', reject);
	});
}

/**
 * 共有にするフォルダーを、毎回新しく作る。AppData の下 (`%TEMP%` も) には作らない。パッケージの中から AppData の下に
 * 書いたものはパッケージごとの場所に回され、外から見えないため
 */
export async function workDir(name) {
	const base = path.join(repoRoot, 'target', 'e2e-msix');
	await fs.mkdir(base, { recursive: true });
	return fs.mkdtemp(path.join(base, `${name}-`));
}
