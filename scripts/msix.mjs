// Windows版の MSIX を作る。`just build`(release exe)の後に呼び、Windows SDK の makeappx で
// dist/weblav-v<version>.msix を作り、試しに入れるための自己署名の証明書で署名する (→ docs/distribution.md「MSIX (Windows)」)。
// 中身と宣言は installer/msix/AppxManifest.xml に書いてある。
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { readVersion } from './cargo-version.mjs';

// 試しに入れるときの発行元。installer/msix/new-test-cert.ps1 が作る証明書の Subject と揃える。
// Store に出すときは、パートナー センターが示す発行元に替える。
const TEST_PUBLISHER = 'CN=WebLAV Test';
const MANIFEST = 'installer/msix/AppxManifest.xml';
const LOGO_DIR = 'assets/msix';
const EXES = ['weblav.exe'];
// makeappx に渡すフォルダ。毎回作り直す。
const LAYOUT_DIR = 'target/msix';
const SDK_BIN = 'C:\\Program Files (x86)\\Windows Kits\\10\\bin';

// MSIX のバージョンは4つ組で、Store は最後を 0 に限る。
function msixVersion(version) {
	const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(version);
	if (!match) {
		throw new Error(`MSIX にできないバージョンです (${version})。x.y.z の形にしてください`);
	}
	return `${match[1]}.${match[2]}.${match[3]}.0`;
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

function layout(version) {
	rmSync(LAYOUT_DIR, { recursive: true, force: true });
	mkdirSync(join(LAYOUT_DIR, 'Assets'), { recursive: true });
	for (const exe of EXES) {
		copyFileSync(join('target/release', exe), join(LAYOUT_DIR, exe));
	}
	for (const logo of readdirSync(LOGO_DIR).filter((name) => name.endsWith('.png'))) {
		copyFileSync(join(LOGO_DIR, logo), join(LAYOUT_DIR, 'Assets', logo));
	}
	const manifest = readFileSync(MANIFEST, 'utf8')
		.replaceAll('{{VERSION}}', msixVersion(version))
		.replaceAll('{{PUBLISHER}}', TEST_PUBLISHER);
	writeFileSync(join(LAYOUT_DIR, 'AppxManifest.xml'), manifest);
}

function main() {
	const version = readVersion();
	const makeappx = findSdkTool('makeappx.exe', 'MAKEAPPX');
	const signtool = findSdkTool('signtool.exe', 'SIGNTOOL');
	const output = `dist/weblav-v${version}.msix`;

	layout(version);
	mkdirSync('dist', { recursive: true });
	execFileSync(makeappx, ['pack', '/o', '/h', 'SHA256', '/d', LAYOUT_DIR, '/p', output], {
		stdio: 'inherit',
	});
	// 証明書は、今の人の証明書ストア (CurrentUser\My) から Subject で選ぶ。
	const subject = TEST_PUBLISHER.replace(/^CN=/, '');
	execFileSync(signtool, ['sign', '/fd', 'SHA256', '/s', 'My', '/n', subject, output], {
		stdio: 'inherit',
	});
	console.log(`wrote ${output}`);
}

main();
