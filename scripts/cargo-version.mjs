// weblav パッケージのバージョンを cargo から読む (scripts/msix.mjs・scripts/bundle-mac.mjs)。
import { execFileSync } from 'node:child_process';

// 手書きの正規表現でCargo.tomlを読まず、cargo自身にバージョンを解決させる
// (workspace継承等、TOMLの書き方が変わってもcargoの方が追随するため)。
export function readVersion() {
	const output = execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1'], {
		encoding: 'utf8',
	});
	const { packages } = JSON.parse(output);
	const pkg = packages.find((p) => p.name === 'weblav');
	if (!pkg) {
		throw new Error('cargo metadataにweblavパッケージが見つかりません');
	}
	return pkg.version;
}
