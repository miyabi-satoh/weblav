// `just spec` から呼ばれるアプリ仕様書ジェネレータ。
//
// 1. 一時 WEBLAV_HOME を作り、専用ポートで `target/debug/weblav-service` を起動する
//    (開発中の dev-backend/:8370 と衝突しないように、かつ既存の開発用DBを汚さないように)。
// 2. seed.sql を node:sqlite で直接流し込む(Rust側の変更なしでシードできる)。
// 3. Playwright でスクリーンショットを撮り、paraglideのメッセージを直接importして文言を取る。
//    各ショットは既定で desktop/tablet/mobile の3サイズを撮る(shots.ts の viewports で
//    絞り込み可能)。3サイズ撮ったものは `{{shot:id}}` の置換先が横並び比較表になる。
// 4. 仕様書の原稿 (環境変数 WEBLAV_SPEC_DIR のフォルダーの *.md) の `{{msg:key}}` `{{shot:id}}` プレースホルダーを実際の値に置換し、
//    docs/generated/spec/ (+ assets/*.png) として書き出す。生成物は .gitignore 対象。
//
// frontend/build は呼び出し元(justfile)が事前にビルド済みである前提
// (rust-embedはdebug buildだと実行時にディスクから読むため、ビルドし忘れると
// 古いUIのままスクショが撮れてしまう)。

import { chromium, type Browser, type Page } from 'playwright';
import { rmSync, mkdirSync, readFileSync, writeFileSync, readdirSync, statSync } from 'node:fs';
import path from 'node:path';
import { startBackend } from './backend-process.ts';
import { setUpFixtures, writeFixtureFiles } from './shot-fixtures.ts';
import { shots, VIEWPORTS, DEFAULT_VIEWPORTS, type ViewportName } from './shots.ts';

const VIEWPORT_LABELS: Record<ViewportName, string> = {
	desktop: 'デスクトップ',
	tablet: 'タブレット',
	mobile: 'モバイル'
};

// paraglideのメッセージ関数群。inputsは{}固定運用のため緩い型で受ける。
type MessageFn = (inputs?: Record<string, unknown>, options?: { locale?: string }) => string;
type Messages = Record<string, MessageFn>;

const REPO_ROOT = path.resolve(import.meta.dirname, '../..');
// 原稿のフォルダーの *.md をそれぞれ docs/generated/spec/<同名> に生成する。
// mo はフォルダー内の複数Markdownをサイドバー切り替えで表示するビューアーのため、
// 単一の巨大ファイルではなくページ単位で分割している。
// 原稿はリポジトリの外に置くので、場所は WEBLAV_SPEC_DIR で受け取る。
// 相対パスは、just のレシピが動くリポジトリの直下から見る。
// 撮影を終えてから気づかないよう、読み込みの時点でフォルダーがあるかを確かめる。
const SPEC_SRC_DIR = path.resolve(REPO_ROOT, process.env.WEBLAV_SPEC_DIR ?? '');
if (
	!process.env.WEBLAV_SPEC_DIR ||
	!statSync(SPEC_SRC_DIR, { throwIfNoEntry: false })?.isDirectory()
) {
	throw new Error('WEBLAV_SPEC_DIR に仕様書の原稿のフォルダーを指定してください');
}
const OUT_DIR = path.join(REPO_ROOT, 'docs/generated/spec');
const ASSETS_DIR = path.join(OUT_DIR, 'assets');

async function main(): Promise<void> {
	writeFixtureFiles();

	// 起動に失敗したら、startBackend がログを出して片付けてから投げる。
	const backend = await startBackend('weblav-spec-');
	const { baseURL } = backend;

	let browser: Browser | undefined;
	const cleanup = async () => {
		await browser?.close();
		await backend.stop();
	};
	process.on('SIGINT', () => {
		cleanup().finally(() => process.exit(130));
	});

	try {
		const fixtures = await setUpFixtures(backend);

		// 前回生成物を消してから作り直す。消さずに追記すると、原稿の側でファイルや
		// shot idを削除・改名した際に古い生成物がいつまでも残ってしまう。
		rmSync(OUT_DIR, { recursive: true, force: true });
		mkdirSync(ASSETS_DIR, { recursive: true });

		browser = await chromium.launch();
		// viewportはページ作成後の`setViewportSize`ではなく、viewportごとに専用の
		// BrowserContextを作って固定する(playwright-coreの型定義が`setViewportSize`は
		// "should only be used for single-page scenarios" と明言しており、1つのpageを
		// 使い回すのは想定外の使い方のため)。ログイン状態はshots.ts側の各ショットが
		// login()/logout()で明示的に管理するので、viewport単位の共有で問題ない。
		const pages = new Map<ViewportName, Page>();
		for (const viewport of Object.keys(VIEWPORTS) as ViewportName[]) {
			// localeを固定するのは、cookieが無ければ表示言語がブラウザーの言語で決まり
			// (→ docs/ui.md「UI 全般」)、日時の書式もそれに従うため。実行するPCの言語で
			// スクショが変わってしまう。
			const context = await browser.newContext({
				viewport: VIEWPORTS[viewport],
				baseURL,
				locale: 'ja-JP'
			});
			// 表示言語のcookieも合わせて置く。localeだけに任せると、前に開いたときのcookieが
			// 残っている環境で揺れる。原稿のプレースホルダー置換もja固定。
			// contextごとに設定する必要がある。
			await context.addCookies([{ name: 'WEBLAV_LOCALE', value: 'ja', url: baseURL }]);
			pages.set(viewport, await context.newPage());
		}

		// id -> viewport順 -> ファイルパス。viewport順は表示テーブルの列順に使う。
		const shotPaths: Record<string, Array<{ viewport: ViewportName; file: string }>> = {};
		for (const [id, entry] of Object.entries(shots)) {
			const viewports = entry.viewports ?? DEFAULT_VIEWPORTS;
			shotPaths[id] = [];
			for (const viewport of viewports) {
				const page = pages.get(viewport)!;
				const suffix = viewports.length > 1 ? `_${viewport}` : '';
				const outFile = path.join(ASSETS_DIR, `${id}${suffix}.png`);
				await entry.run(page, { baseURL, outFile, fixtures });
				shotPaths[id].push({ viewport, file: outFile });
				console.log(`[shot] ${id} (${viewport}) -> ${path.relative(REPO_ROOT, outFile)}`);
			}
		}

		// paraglideメッセージを直接importして文言を取得する(ja固定)。
		// esModuleInterop によりモジュール名前空間の型に合成の`default`プロパティが
		// 付き、`Messages`(Record<string, MessageFn>)と構造的に一致しなくなるため
		// キャストする。実際に関数かどうかは下の`missing`チェックで検証済み。
		const messages = (await import('../src/lib/paraglide/messages.js')) as unknown as Messages;

		const PARAM_NAMES = new Proxy({} as Record<string, string>, {
			get: (_, key) => (typeof key === 'string' ? `{${key}}` : undefined)
		});

		const missing: string[] = [];
		const srcFiles = readdirSync(SPEC_SRC_DIR).filter((f) => f.endsWith('.md'));
		for (const file of srcFiles) {
			const template = readFileSync(path.join(SPEC_SRC_DIR, file), 'utf-8');
			const rendered = template.replace(
				/\{\{(msg|shot):([a-zA-Z0-9_]+)\}\}/g,
				(full, kind, key) => {
					if (kind === 'msg') {
						const fn = messages[key];
						if (typeof fn !== 'function') {
							missing.push(`${file}: ${full}`);
							return full;
						}
						// 差し込みの値は持たないので、`{name}` のように引数の名前をそのまま出す。
						return fn(PARAM_NAMES, { locale: 'ja' });
					}
					// kind === 'shot'
					if (!(key in shotPaths)) {
						missing.push(`${file}: ${full}`);
						return full;
					}
					const entries = shotPaths[key];
					if (entries.length === 1) {
						const rel = path.relative(OUT_DIR, entries[0].file);
						return `![${key}](${rel})`;
					}
					// 複数viewport -> 横並び比較表(GFMテーブル)にする。
					const header = `| ${entries.map((e) => VIEWPORT_LABELS[e.viewport]).join(' | ')} |`;
					const sep = `|${entries.map(() => ' --- ').join('|')}|`;
					const row = `| ${entries
						.map((e) => `![${key}_${e.viewport}](${path.relative(OUT_DIR, e.file)})`)
						.join(' | ')} |`;
					return `${header}\n${sep}\n${row}`;
				}
			);
			writeFileSync(path.join(OUT_DIR, file), rendered);
		}

		if (missing.length > 0) {
			throw new Error(`未解決のプレースホルダーがあります: ${missing.join(', ')}`);
		}

		console.log(`生成しました: ${path.relative(REPO_ROOT, OUT_DIR)}/ (${srcFiles.length}ファイル)`);
	} finally {
		await cleanup();
	}
}

main().catch((err) => {
	console.error(err);
	process.exit(1);
});
