// macOS 版の .app を組み立てる (署名は呼ぶ側が付ける)。手元で常用する版 (scripts/bundle-mac.mjs) と
// 配る版 (scripts/release-mac.mjs) で同じ組み立て方にするため、ここに1つだけ置く。
import { execFileSync } from 'node:child_process';
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import sharp from 'sharp';

export const INFO_PLIST = 'installer/macos/Info.plist';
const SVG_PATH = 'assets/icon.svg';
// iconutil が求める名前とサイズ (icon_<size>x<size>[@2x].png)。
const ICON_SIZES = [16, 32, 128, 256, 512];
// assets/icon.svg の viewBox は 32×32。scripts/generate-icon.mjs と同じく density で拡大する。
const densityFor = (size) => Math.round(72 * (size / 32));

/** 実行ファイルの名前。Info.plist の CFBundleExecutable と揃える。 */
export const EXECUTABLE = 'weblav';
export const ENTITLEMENTS = 'installer/macos/weblav.entitlements';

/**
 * ビルド番号。build.rs が実行ファイルに入れる値 (→ docs/distribution.md「版の番号」) と同じ数え方にし、
 * Info.plist の CFBundleVersion に入れる。App Store Connect は同じビルド番号を2度受け付けないが、
 * コミットが進めば増えるので、版 (CFBundleShortVersionString) を上げずに上げ直せる。
 */
export function readBuildNumber() {
	return execFileSync('git', ['rev-list', '--count', 'HEAD'], { encoding: 'utf8' }).trim();
}

async function writeIcns(path, workDir) {
	const iconset = join(workDir, 'weblav.iconset');
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

/** `binary` を実行ファイルにした `appDir` (…/WebLAV.app) を作り直す。 */
export async function assembleApp({ binary, appDir, version, build }) {
	rmSync(appDir, { recursive: true, force: true });
	const macos = join(appDir, 'Contents/MacOS');
	const resources = join(appDir, 'Contents/Resources');
	mkdirSync(macos, { recursive: true });
	mkdirSync(resources, { recursive: true });

	cpSync(binary, join(macos, EXECUTABLE));
	const plist = readFileSync(INFO_PLIST, 'utf8')
		.replaceAll('{{VERSION}}', version)
		.replaceAll('{{BUILD}}', build);
	writeFileSync(join(appDir, 'Contents/Info.plist'), plist);
	await writeIcns(join(resources, 'weblav.icns'), join(appDir, '..'));
}
