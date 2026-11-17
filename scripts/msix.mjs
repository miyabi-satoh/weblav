// Windows版の MSIX を作る。`just build`(release exe)の後に呼び、Windows SDK の makeappx で
// dist/weblav-v<version>.msix を作り、試しに入れるための自己署名の証明書で署名する (→ docs/distribution.md「MSIX (Windows)」)。
// `--store` を付けると、Store に上げる dist/weblav-v<version>-store.msix を、Store の発行元で署名せずに作る。
// `--install` を付けると、作った試しの版をこの PC に入れ直して起動し直す (`just install-windows`)。
// `--e2e --revision=<n>` は、MSIX 版の確認 (`just msix-check`) が入れる版を target/msix-e2e/ に作る。
// 常用の版と並べて入れられるよう、パッケージの名前と表示名だけを替える (→ e2e/README.md「MSIX 版の確認」)。
// 中身と宣言は installer/msix/AppxManifest.xml に書いてある。
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { readVersion } from './cargo-version.mjs';

// 試しに入れるときの発行元。installer/msix/new-test-cert.ps1 が作る証明書の Subject と揃える。
const TEST_PUBLISHER = 'CN=WebLAV Test';
// Store での発行元。パートナー センターの製品の「Product identity」に出る値。
// Store に上げる版は Store が署名するので、自分では署名しない (この発行元の証明書は手元に無い)。
const STORE_PUBLISHER = 'CN=BA27F417-AAC4-43D9-9E55-3320F7F52C6F';
const MANIFEST = 'installer/msix/AppxManifest.xml';
const LOGO_DIR = 'assets/msix';
const EXES = ['weblav.exe'];
// makeappx に渡すフォルダー。毎回作り直す。
const LAYOUT_DIR = 'target/msix';
const SDK_BIN = 'C:\\Program Files (x86)\\Windows Kits\\10\\bin';

// MSIX のバージョンは4つ組で、Store は最後を 0 に限る。
// 試しの版は最後をコミットの数にする。同じ版番号のままでは上から入れられず、外すとデータが消えるため
// (この PC に入れるときは installRevision)。
function msixVersion(version, store, revisionOverride) {
	const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(version);
	if (!match) {
		throw new Error(`MSIX にできないバージョンです (${version})。x.y.z の形にしてください`);
	}
	const revision =
		revisionOverride ??
		(store ? '0' : String(commitCount()));
	return `${match[1]}.${match[2]}.${match[3]}.${revision}`;
}

function commitCount() {
	return Number(execFileSync('git', ['rev-list', '--count', 'HEAD'], { encoding: 'utf8' }).trim());
}

// Windows SDK の中で、いちばん新しい版の x64 のツールを探す。別の場所なら環境変数 (MAKEAPPX・SIGNTOOL) で指定する。
function findSdkTool(name, envName) {
	const fromEnv = process.env[envName];
	if (fromEnv) {
		return fromEnv;
	}
	const versions = existsSync(SDK_BIN)
		? readdirSync(SDK_BIN)
				.filter((dir) => /^10\.[\d.]+$/.test(dir))
				.sort((a, b) => a.localeCompare(b, undefined, { numeric: true }))
				.reverse()
		: [];
	for (const version of versions) {
		const path = join(SDK_BIN, version, 'x64', name);
		if (existsSync(path)) {
			return path;
		}
	}
	throw new Error(
		`${name} が見つかりません。Windows SDK を入れるか、環境変数 ${envName} にパスを指定してください`,
	);
}

// 確かめ用の版のパッケージの名前と表示名。常用の版 (amiiby.WebLAV) と別のパッケージにして、データ・ログイン時の起動・
// ファイアウォールの規則を常用の版と分ける。e2e/lib/windows.mjs の PACKAGE_NAME と揃える。
const E2E_NAME = 'amiiby.WebLAV.E2E';
const E2E_DISPLAY_NAME = 'WebLAV E2E';

function layout(version, store, { e2e = false, revision } = {}) {
	rmSync(LAYOUT_DIR, { recursive: true, force: true });
	mkdirSync(join(LAYOUT_DIR, 'Assets'), { recursive: true });
	for (const exe of EXES) {
		copyFileSync(join('target/release', exe), join(LAYOUT_DIR, exe));
	}
	for (const logo of readdirSync(LOGO_DIR).filter((name) => name.endsWith('.png'))) {
		copyFileSync(join(LOGO_DIR, logo), join(LAYOUT_DIR, 'Assets', logo));
	}
	let manifest = readFileSync(MANIFEST, 'utf8')
		.replaceAll('{{VERSION}}', msixVersion(version, store, revision))
		.replaceAll('{{PUBLISHER}}', store ? STORE_PUBLISHER : TEST_PUBLISHER);
	if (e2e) {
		manifest = manifest
			.replace('Name="amiiby.WebLAV"', `Name="${E2E_NAME}"`)
			.replaceAll('DisplayName="WebLAV"', `DisplayName="${E2E_DISPLAY_NAME}"`)
			.replace('<DisplayName>WebLAV</DisplayName>', `<DisplayName>${E2E_DISPLAY_NAME}</DisplayName>`);
	}
	writeFileSync(join(LAYOUT_DIR, 'AppxManifest.xml'), manifest);
}

function main() {
	const store = process.argv.includes('--store');
	const e2e = process.argv.includes('--e2e');
	const revision = process.argv.find((arg) => arg.startsWith('--revision='))?.slice('--revision='.length);
	if (e2e && (store || !/^\d+$/.test(revision ?? ''))) {
		throw new Error('--e2e には --revision=<数字> を付け、--store とは一緒に使わないでください');
	}
	const version = readVersion();
	const makeappx = findSdkTool('makeappx.exe', 'MAKEAPPX');
	const output = e2e
		? `target/msix-e2e/weblav-e2e-${revision}.msix`
		: `dist/weblav-v${version}${store ? '-store' : ''}.msix`;

	layout(version, store, {
		e2e,
		revision: revision ?? (!store && process.argv.includes('--install') ? installRevision() : undefined),
	});
	mkdirSync(dirname(output), { recursive: true });
	execFileSync(makeappx, ['pack', '/o', '/h', 'SHA256', '/d', LAYOUT_DIR, '/p', output], {
		stdio: 'inherit',
	});
	if (store) {
		console.log(`wrote ${output} (unsigned)`);
		return;
	}
	const signtool = findSdkTool('signtool.exe', 'SIGNTOOL');
	// 証明書は、今の人の証明書ストア (CurrentUser\My) から Subject で選ぶ。
	const subject = TEST_PUBLISHER.replace(/^CN=/, '');
	execFileSync(signtool, ['sign', '/fd', 'SHA256', '/s', 'My', '/n', subject, output], {
		stdio: 'inherit',
	});
	console.log(`wrote ${output}`);
	if (process.argv.includes('--install')) {
		install(output);
	}
}

// この PC に入れる版の最後の番号。ふだんはコミットの数で、今入っている版と同じならその次にする。
// Windows は、同じ版の番号で中身の違うパッケージを上から入れるのを断る (コミット前の直しを入れ直すときに起きる)。
function installRevision() {
	const commits = commitCount();
	const installed = execFileSync(
		'pwsh',
		[
			'-NoProfile',
			'-Command',
			`(Get-AppxPackage -Name amiiby.WebLAV | Where-Object Publisher -eq '${TEST_PUBLISHER}').Version`,
		],
		{ encoding: 'utf8' },
	).trim();
	const installedRevision = installed ? Number(installed.split('.')[3]) : undefined;
	return String(commits === installedRevision ? commits + 1 : commits);
}

// 上から入れる (外さないので、データとログイン時の起動の設定が残る)。動いている WebLAV は止め、入れた後に起動し直す。
// 版の最後の番号はコミットの数なので、作業中のブランチの版を入れた後にスカッシュマージした main を入れると番号が下がる。
// Windows は番号の下がる入れ直しを断るので、-ForceUpdateFromAnyVersion で下がる向きも許す。
// 入れられなかったときに、前から入っている版を「入れた」と出して成功で終わらないよう、失敗で止める。
function install(output) {
	const script = [
		"$ErrorActionPreference = 'Stop'",
		`Add-AppxPackage -Path '${output}' -ForceApplicationShutdown -ForceUpdateFromAnyVersion`,
		`$p = Get-AppxPackage -Name amiiby.WebLAV | Where-Object Publisher -eq '${TEST_PUBLISHER}'`,
		'$id = ($p | Get-AppxPackageManifest).Package.Applications.Application.Id',
		'Start-Process "shell:AppsFolder\\$($p.PackageFamilyName)!$id"',
		'Write-Output "installed $($p.Version)"',
	].join('; ');
	execFileSync('pwsh', ['-NoProfile', '-Command', script], { stdio: 'inherit' });
}

main();
