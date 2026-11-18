// Mac 版の配る版を作る (→ docs/distribution.md「Mac 版の配る版」)。
// `just release-mas`: ユニバーサル版を Mac App Store 向けに署名して pkg にし、App Store Connect に上げる。
// `just release-mac`: ユニバーサル版を Developer ID で署名・公証し、DMG を target/release-mac/<版>/ に置く。
//   Mac App Store の審査が通らないときだけ使う。
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { parseArgs } from 'node:util';
import { readVersion } from './cargo-version.mjs';
import { ENTITLEMENTS, EXECUTABLE, assembleApp, readBuildNumber } from './mac-app.mjs';

// 証明書とプロファイルの Team ID (developer.apple.com/account の Membership details)
const TEAM_ID = 'V6W6CVP93A';
const BUNDLE_ID = 'com.amiiby.weblav';
const RUST_TARGETS = ['aarch64-apple-darwin', 'x86_64-apple-darwin'];
// Store の版と Store の外の版を、同じ版でも互いに消さないよう分けて置く
const APP_STORE_OUT_ROOT = 'target/release-mas';
const DEVELOPER_ID_OUT_ROOT = 'target/release-mac';

function run(command, args, options = {}) {
	execFileSync(command, args, { stdio: 'inherit', ...options });
}

function read(command, args, options = {}) {
	return execFileSync(command, args, { encoding: 'utf8', ...options }).trim();
}

function fail(message) {
	console.error(message);
	process.exit(1);
}

// 端末に出さずにパスワードを読む。貼り付けは1回の data でまとめて届く
function askSecret(label) {
	const { stdin, stdout } = process;
	if (!stdin.isTTY) {
		fail(`${label}を読めません。端末から実行してください`);
	}
	stdout.write(`${label}: `);
	stdin.setRawMode(true);
	stdin.setEncoding('utf8');
	stdin.resume();
	return new Promise((resolve) => {
		let value = '';
		const onData = (chunk) => {
			for (const char of chunk) {
				if (char === '\r' || char === '\n') {
					stdin.setRawMode(false);
					stdin.pause();
					stdin.off('data', onData);
					stdout.write('\n');
					resolve(value);
					return;
				}
				if (char === '\u0003') {
					stdout.write('\n');
					process.exit(130);
				}
				value = char === '\u007f' ? value.slice(0, -1) : value + char;
			}
		};
		stdin.on('data', onData);
	});
}

// キーチェーンにある証明書の名前。インストーラーの証明書はコード署名の用途に出ないので、policy で分ける
function identity(prefixes, policy) {
	const identities = read('security', ['find-identity', '-v', ...(policy ? ['-p', policy] : [])]);
	for (const prefix of prefixes) {
		const match = new RegExp(`"(${prefix}: [^"]+\\(${TEAM_ID}\\))"`).exec(identities);
		if (match) {
			return match[1];
		}
	}
	fail(
		`キーチェーンに ${prefixes[0]} の証明書がありません。要るものと作り方は docs/distribution.md「Mac 版の配る版」にあります`
	);
}

function requireAppleId(purpose) {
	const appleId = process.env.APPLE_ID;
	if (!appleId) {
		fail(`APPLE_ID に、${purpose}に使う Apple Account のメールアドレスを入れてください`);
	}
	return appleId;
}

function freshOutDir(root, version) {
	const outDir = join(root, version);
	rmSync(outDir, { recursive: true, force: true });
	mkdirSync(outDir, { recursive: true });
	return outDir;
}

// 画面は1回だけ作り、両方の CPU 向けの実行ファイルに埋め込む (→ docs/distribution.md「バージョン・アイコンの埋め込み」)
function buildUniversalBinary(outDir) {
	const installed = read('rustup', ['target', 'list', '--installed']).split('\n');
	const missing = RUST_TARGETS.filter((target) => !installed.includes(target));
	if (missing.length > 0) {
		run('rustup', ['target', 'add', ...missing]);
	}
	run('just', ['frontend-build']);
	for (const target of RUST_TARGETS) {
		run('cargo', ['build', '--release', '--bin', EXECUTABLE, '--target', target]);
	}
	const universal = join(outDir, EXECUTABLE);
	run('lipo', [
		'-create',
		...RUST_TARGETS.map((target) => join('target', target, 'release', EXECUTABLE)),
		'-output',
		universal
	]);
	const archs = read('lipo', ['-archs', universal]).split(' ');
	if (!archs.includes('x86_64') || !archs.includes('arm64')) {
		fail(`ユニバーサル版になっていません (${archs.join(' ')})`);
	}
	return universal;
}

// 別の App ID・チーム・OS のプロファイルだと、上げた後に断られるので、ビルドの前に確かめる。
// developer.apple.com では iPhone 用の「App Store Connect」と Mac 用の「Mac App Store Connect」が並び、取り違えやすい
function checkProfile(profile) {
	const plist = execFileSync('security', ['cms', '-D', '-i', profile]);
	const value = (key) =>
		read('/usr/libexec/PlistBuddy', ['-c', `Print :${key}`, '/dev/stdin'], { input: plist });
	const platform = value('Platform:0');
	if (platform !== 'OSX') {
		fail(`プロビジョニングプロファイルが ${platform} 用です (Mac App Store Connect のものが要ります)`);
	}
	const appId = value('Entitlements:com.apple.application-identifier');
	if (appId !== `${TEAM_ID}.${BUNDLE_ID}`) {
		fail(
			`プロビジョニングプロファイルの App ID が ${appId} です (${TEAM_ID}.${BUNDLE_ID} のものが要ります)`
		);
	}
}

// upload: false は上げずに pkg までを作る (作り方を変えたときに、ビルド番号を使わずに試す)
async function buildAppStore({ upload }) {
	const version = readVersion();
	const build = readBuildNumber();
	const signing = identity(['Apple Distribution'], 'codesigning');
	// Mac Installer Distribution は、キーチェーンでは古い名前で出ることがある
	const installer = identity(['Mac Installer Distribution', '3rd Party Mac Developer Installer']);
	const profile = process.env.APPLE_PROVISIONING_PROFILE;
	if (!profile || !existsSync(profile)) {
		fail(
			'APPLE_PROVISIONING_PROFILE に、Mac App Store Connect のプロビジョニングプロファイルのパスを入れてください'
		);
	}
	checkProfile(profile);
	// ビルドの後でなく前に聞く。端末が無いと分かるのが、数分のビルドの後にならないため
	const credentials = upload
		? {
				appleId: requireAppleId('アップロード'),
				password: await askSecret('アップロードのアプリ用パスワード')
			}
		: undefined;
	const outDir = freshOutDir(APP_STORE_OUT_ROOT, version);
	const app = join(outDir, 'WebLAV.app');
	await assembleApp({ binary: buildUniversalBinary(outDir), appDir: app, version, build });

	// プロファイルと結ぶ権限は、プロファイルを埋め込まない版 (just bundle-mac・Developer ID) に付けると起動できないので、
	// Store の版のときだけ、ほかの版と同じ権限に足して使う
	const entitlements = resolve(outDir, 'Entitlements.plist');
	copyFileSync(ENTITLEMENTS, entitlements);
	// plutil は鍵の名前の「.」を階層の区切りと読むので、「:」で区切る PlistBuddy で足す
	run('/usr/libexec/PlistBuddy', [
		'-c',
		`Add :com.apple.application-identifier string ${TEAM_ID}.${BUNDLE_ID}`,
		'-c',
		`Add :com.apple.developer.team-identifier string ${TEAM_ID}`,
		entitlements
	]);
	copyFileSync(profile, join(app, 'Contents/embedded.provisionprofile'));
	run('codesign', ['--force', '--sign', signing, '--entitlements', entitlements, app]);

	const signed = read('codesign', ['-d', '--entitlements', '-', '--xml', app]);
	for (const key of [
		'com.apple.security.app-sandbox',
		'com.apple.application-identifier',
		'com.apple.developer.team-identifier'
	]) {
		if (!signed.includes(key)) {
			fail(`App Store 向けの権限で署名されていません (${key} がありません)`);
		}
	}
	run('codesign', ['--verify', '--strict', '--deep', app]);

	const pkg = join(outDir, `WebLAV_${version}_${build}_universal.pkg`);
	run('xcrun', ['productbuild', '--sign', installer, '--component', app, '/Applications', pkg]);
	run('pkgutil', ['--check-signature', pkg]);
	if (!credentials) {
		console.log(`\n${pkg} に置きました (上げていません)。`);
		return;
	}

	run(
		'xcrun',
		[
			'altool',
			'--upload-app',
			'-f',
			pkg,
			'-t',
			'macos',
			'-u',
			credentials.appleId,
			'-p',
			'@env:APPLE_PASSWORD'
		],
		{ env: { ...process.env, APPLE_PASSWORD: credentials.password } }
	);
	console.log(
		`\n${pkg} (版 ${version}・ビルド ${build}) を App Store Connect に上げました。処理が済むと TestFlight に出ます。`
	);
}

async function buildDeveloperId() {
	const version = readVersion();
	const build = readBuildNumber();
	const signing = identity(['Developer ID Application'], 'codesigning');
	const appleId = requireAppleId('公証');
	const password = await askSecret('公証のアプリ用パスワード');
	const outDir = freshOutDir(DEVELOPER_ID_OUT_ROOT, version);
	const app = join(outDir, 'WebLAV.app');
	await assembleApp({ binary: buildUniversalBinary(outDir), appDir: app, version, build });

	// 公証は hardened runtime と時刻の印を求める
	run('codesign', [
		'--force',
		'--options',
		'runtime',
		'--timestamp',
		'--sign',
		signing,
		'--entitlements',
		ENTITLEMENTS,
		app
	]);
	const dmg = join(outDir, `WebLAV_${version}_universal.dmg`);
	run('hdiutil', ['create', '-volname', 'WebLAV', '-srcfolder', app, '-ov', '-format', 'UDZO', dmg]);
	run('codesign', ['--force', '--timestamp', '--sign', signing, dmg]);
	run(
		'xcrun',
		[
			'notarytool',
			'submit',
			dmg,
			'--apple-id',
			appleId,
			'--team-id',
			TEAM_ID,
			'--password',
			password,
			'--wait'
		],
		{ stdio: 'inherit' }
	);
	run('xcrun', ['stapler', 'staple', dmg]);
	// 公証の結果が綴じてあり、Gatekeeper が通すことを確かめる
	run('xcrun', ['stapler', 'validate', dmg]);
	run('spctl', ['--assess', '--type', 'open', '--context', 'context:primary-signature', '--verbose=2', dmg]);
	rmSync(app, { recursive: true, force: true });
	console.log(`\n${dmg} に置きました。`);
}

// 打ち間違えた引数を黙って捨てると、試すつもりで上げてしまうので止める
let options;
try {
	({ values: options } = parseArgs({
		options: { 'app-store': { type: 'boolean' }, 'no-upload': { type: 'boolean' } },
		strict: true
	}));
} catch (error) {
	fail(error.message);
}
if (options['no-upload'] && !options['app-store']) {
	fail('--no-upload は --app-store と一緒に使います');
}
if (options['app-store']) {
	await buildAppStore({ upload: !options['no-upload'] });
} else {
	await buildDeveloperId();
}
