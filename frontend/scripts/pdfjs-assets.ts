// PDF.js が描画中に取りに行くファイル (日本語の CMap・画像の展開用の wasm・ICC) を
// 配る物に入れる Vite プラグイン (→ docs/ui.md「PDF・動画・テキストのビューア」)。
// どれも import で辿れないので、置き場ごと写す。置き場の URL は `virtual:pdfjs-assets` で渡す。
// ADR: 標準フォントの置き場 (`standard_fonts`) は写さない。Liberation が GPL-2.0 で、配る物に入れられないため。
// 埋め込まれていない標準フォントは、ブラウザのフォントで描く (PDF.js の `useSystemFonts` の既定)。

import { readdirSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import type { Plugin } from 'vite';

const VIRTUAL_ID = 'virtual:pdfjs-assets';
const RESOLVED_VIRTUAL_ID = `\0${VIRTUAL_ID}`;

/** 写す置き場。PDF.js の `getDocument` の `cMapUrl` などに1つずつ渡す。 */
const PDFJS_ASSET_DIRS = ['cmaps', 'wasm', 'iccs'] as const;

// 写さないファイル。
// - 条文: 画面のライセンス表示に載せる (third-party-licenses.ts の EXTRA_TEXT_FILES)。
// - `*_nowasm_fallback.js`: WebAssembly を使えないブラウザ向け。対象のブラウザはどれも使える。
// - `quickjs-eval.*`: PDF の中のスクリプトを動かすためのもの。スクリプトは動かさない。
const SKIP = /^LICENSE|_nowasm_fallback\.js$|^quickjs-eval\./;

function pdfjsDir(root: string): string {
	return dirname(createRequire(join(root, 'package.json')).resolve('pdfjs-dist/package.json'));
}

export function pdfjsAssets(): Plugin {
	let root = '';
	let base = '';
	return {
		name: 'weblav:pdfjs-assets',
		configResolved(config) {
			root = config.root;
			const { version } = JSON.parse(readFileSync(join(pdfjsDir(root), 'package.json'), 'utf8'));
			// 版ごとに置き場を分け、`_app/immutable/` の長いキャッシュに乗せる (→ src/static_files.rs)。
			base = `/_app/immutable/pdfjs-${version}/`;
		},
		resolveId(id) {
			return id === VIRTUAL_ID ? RESOLVED_VIRTUAL_ID : undefined;
		},
		load(id) {
			if (id !== RESOLVED_VIRTUAL_ID) return;
			const urls = Object.fromEntries(PDFJS_ASSET_DIRS.map((dir) => [dir, `${base}${dir}/`]));
			return `export default ${JSON.stringify(urls)};`;
		},
		configureServer(server) {
			server.middlewares.use((req, res, next) => {
				const path = req.url?.split('?')[0] ?? '';
				if (!path.startsWith(base)) return next();
				let decoded: string;
				try {
					decoded = decodeURIComponent(path.slice(base.length));
				} catch {
					return next();
				}
				const [dir, ...rest] = decoded.split('/');
				const file = rest.join('/');
				// 配る物に写すファイルだけを返す。名前を実在の一覧と突き合わせ、置き場の外 (`..`) を読ませない。
				if (!(PDFJS_ASSET_DIRS as readonly string[]).includes(dir) || SKIP.test(file))
					return next();
				const source = join(pdfjsDir(root), dir);
				if (!readdirSync(source).includes(file)) return next();
				res.end(readFileSync(join(source, file)));
			});
		},
		generateBundle() {
			if (this.environment.name !== 'client') return;
			for (const dir of PDFJS_ASSET_DIRS) {
				const source = join(pdfjsDir(root), dir);
				for (const file of readdirSync(source)) {
					if (SKIP.test(file)) continue;
					this.emitFile({
						type: 'asset',
						fileName: `${base.slice(1)}${dir}/${file}`,
						source: readFileSync(join(source, file))
					});
				}
			}
		}
	};
}
