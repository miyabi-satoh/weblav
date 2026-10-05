// macOS 版の .app を作る。`just build`(release バイナリ)の後に呼び、
// target/release/bundle/WebLAV.app を組み立てて、サンドボックスの権限 (entitlements) 付きで
// 証明書なしの署名 (ad-hoc) を付ける (→ docs/distribution.md「ビルド・配布の方法」)。
// 公証はしていない。
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import sharp from 'sharp';
import { readVersion } from './cargo-version.mjs';

const APP_DIR = 'target/release/bundle/WebLAV.app';
const INFO_PLIST = 'installer/macos/Info.plist';
const ENTITLEMENTS = 'installer/macos/weblav.entitlements';
const SVG_PATH = 'assets/icon.svg';
const BINARIES = ['weblav'];
// iconutil が求める名前とサイズ (icon_<size>x<size>[@2x].png)。
const ICON_SIZES = [16, 32, 128, 256, 512];
// assets/icon.svg の viewBox は 32×32。scripts/generate-icon.mjs と同じく density で拡大する。
const densityFor = (size) => Math.round(72 * (size / 32));

async function writeIcns(path) {
	const iconset = 'target/release/bundle/weblav.iconset';
	rmSync(iconset, { recursive: true, force: true });
	mkdirSync(iconset, { recursive: true });
	const svg = readFileSync(SVG_PATH);
	for (const size of ICON_SIZES) {
		for (const scale of [1, 2]) {
			const px = size * scale;
			const name = `icon_${size}x${size}${scale === 2 ? '@2x' : ''}.png`;
			await sharp(svg, { density: densityFor(px) })
				.resize(px, px)
				.png()
				.toFile(join(iconset, name));
		}
	}
	execFileSync('iconutil', ['-c', 'icns', iconset, '-o', path]);
	rmSync(iconset, { recursive: true, force: true });
}

async function main() {
	const version = readVersion();
	rmSync(APP_DIR, { recursive: true, force: true });
	const macos = join(APP_DIR, 'Contents/MacOS');
	const resources = join(APP_DIR, 'Contents/Resources');
	mkdirSync(macos, { recursive: true });
	mkdirSync(resources, { recursive: true });

	for (const bin of BINARIES) {
		cpSync(join('target/release', bin), join(macos, bin));
	}
	const plist = readFileSync(INFO_PLIST, 'utf8').replaceAll('{{VERSION}}', version);
	writeFileSync(join(APP_DIR, 'Contents/Info.plist'), plist);
	await writeIcns(join(resources, 'weblav.icns'));

	execFileSync(
		'codesign',
		['--force', '--deep', '--sign', '-', '--entitlements', ENTITLEMENTS, APP_DIR],
		{ stdio: 'inherit' }
	);
	console.log(`wrote ${APP_DIR} (v${version})`);
}

await main();
