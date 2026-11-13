import { paraglideVitePlugin } from '@inlang/paraglide-js';
import tailwindcss from '@tailwindcss/vite';
import { configDefaults, defineConfig } from 'vitest/config';
import { playwright } from '@vitest/browser-playwright';
import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { pdfjsAssets } from './scripts/pdfjs-assets.ts';
import { thirdPartyLicenses } from './scripts/third-party-licenses.ts';
import { flakyAnnotations } from './scripts/vitest-flaky-annotations.ts';

export default defineConfig({
	plugins: [
		tailwindcss(),
		thirdPartyLicenses(),
		pdfjsAssets(),
		sveltekit({
			compilerOptions: {
				// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			adapter: adapter({
				fallback: 'index.html'
			}),
			typescript: {
				// 既定の include は src/・test/ 配下のみで scripts/・playwright.config.ts が
				// tsc/svelte-check の対象外になっていた (→ frontend/scripts, playwright.config.ts)。
				config: (config) => {
					config.include.push('../scripts/**/*.ts', '../playwright.config.ts');
				}
			}
		}),

		paraglideVitePlugin({
			project: './project.inlang',
			outdir: './src/lib/paraglide',
			emitTsDeclarations: true,
			// 表示言語の決まり方は → docs/ui.md「UI 全般」。
			// `globalVariable` (既定の strategy に入っている) は落とす。cookie が先にあるので
			// 読まれる場面が無く、保存と表示の追随は cookie と `setLocale` のリロードで足りる。
			strategy: ['cookie', 'preferredLanguage', 'baseLocale'],
			cookieName: 'WEBLAV_LOCALE'
		})
	],
	server: {
		// `pnpm run dev` 時、/api への呼び出しは weblav.exe の axum サーバー(固定ポート)へ委譲する。
		proxy: {
			'/api': 'http://127.0.0.1:8370'
		}
	},
	test: {
		expect: { requireAssertions: true },
		// 落ちたテストを2回まで流し直す (Playwright の CI の既定と同じ回数)。機械が詰まって時々落ちるテストで、CI や手元の検査が止まらないようにする。
		// 流し直して通ったテストは flakyAnnotations が最後に警告で出すので、埋もれずに直す相手として見える。
		retry: 2,
		// 場所を出せば、警告から行へ飛べる。
		includeTaskLocation: true,
		// reporters を書くと既定が置き換わるので、既定 (エージェントでは minimal、GitHub Actions では github-actions も) に足す。
		reporters: [...configDefaults.reporters, flakyAnnotations()],
		projects: [
			{
				extends: './vite.config.ts',
				test: {
					name: 'client',
					browser: {
						enabled: true,
						// 表示言語はブラウザーの言語で決まるので、実行する PC の言語で結果が変わらないよう固定する。
						provider: playwright({ contextOptions: { locale: 'ja-JP' } }),
						instances: [{ browser: 'chromium', headless: true }]
					},
					include: ['src/**/*.svelte.{test,spec}.{js,ts}'],
					exclude: ['src/lib/server/**']
				}
			},

			{
				extends: './vite.config.ts',
				test: {
					name: 'server',
					environment: 'node',
					include: ['src/**/*.{test,spec}.{js,ts}'],
					exclude: ['src/**/*.svelte.{test,spec}.{js,ts}']
				}
			}
		]
	}
});
