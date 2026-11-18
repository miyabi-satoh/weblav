// macOS 版の .app を作る。`just build`(release バイナリ)の後に呼び、
// target/release/bundle/WebLAV.app を組み立てて、サンドボックスの権限 (entitlements) 付きで
// 証明書なしの署名 (ad-hoc) を付ける。開発する人の手元で常用するためのもので、配る版は
// scripts/release-mac.mjs が作る (→ docs/distribution.md「ビルド・配布の方法」)。
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { readVersion } from './cargo-version.mjs';
import { ENTITLEMENTS, EXECUTABLE, assembleApp, readBuildNumber } from './mac-app.mjs';

const APP_DIR = 'target/release/bundle/WebLAV.app';

const version = readVersion();
await assembleApp({
	binary: join('target/release', EXECUTABLE),
	appDir: APP_DIR,
	version,
	build: readBuildNumber()
});
execFileSync('codesign', ['--force', '--sign', '-', '--entitlements', ENTITLEMENTS, APP_DIR], {
	stdio: 'inherit'
});
console.log(`wrote ${APP_DIR} (v${version})`);
