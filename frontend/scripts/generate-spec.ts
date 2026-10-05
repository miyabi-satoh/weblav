// `just spec` から呼ばれるアプリ仕様書ジェネレータ。
//
// 1. 一時 WEBLAV_HOME を作り、専用ポートで `target/debug/weblav-service` を起動する
//    (開発中の dev-backend/:3000 と衝突しないように、かつ既存の開発用DBを汚さないように)。
// 2. seed.sql を node:sqlite で直接流し込む(Rust側の変更なしでシードできる)。
// 3. Playwright でスクリーンショットを撮り、paraglideのメッセージを直接importして文言を取る。
//    各ショットは既定で desktop/tablet/mobile の3サイズを撮る(shots.ts の viewports で
//    絞り込み可能)。3サイズ撮ったものは `{{shot:id}}` の置換先が横並び比較表になる。
// 4. 仕様書の原稿 (環境変数 WEBLAV_SPEC_DIR のフォルダの *.md) の `{{msg:key}}` `{{shot:id}}` プレースホルダーを実際の値に置換し、
//    docs/generated/spec/ (+ assets/*.png) として書き出す。生成物は .gitignore 対象。
//
// frontend/build は呼び出し元(justfile)が事前にビルド済みである前提
// (rust-embedはdebug buildだと実行時にディスクから読むため、ビルドし忘れると
// 古いUIのままスクショが撮れてしまう)。

import { chromium, type Browser, type Page } from 'playwright';
import type { DatabaseSync } from 'node:sqlite';
import {
	rmSync,
	mkdirSync,
	readFileSync,
	writeFileSync,
	readdirSync,
	utimesSync,
	statSync
} from 'node:fs';
import path from 'node:path';
import { crc32, deflateSync } from 'node:zlib';
import { startBackend, withDatabase } from './backend-process.ts';
import { shots, VIEWPORTS, DEFAULT_VIEWPORTS, type ViewportName } from './shots.ts';
import { TEST_ADMIN } from './test-account.ts';

const VIEWPORT_LABELS: Record<ViewportName, string> = {
	desktop: 'デスクトップ',
	tablet: 'タブレット',
	mobile: 'モバイル'
};

// paraglideのメッセージ関数群。inputsは{}固定運用のため緩い型で受ける。
type MessageFn = (inputs?: Record<string, unknown>, options?: { locale?: string }) => string;
type Messages = Record<string, MessageFn>;

const REPO_ROOT = path.resolve(import.meta.dirname, '../..');
const SEED_SQL_PATH = path.join(import.meta.dirname, 'seed.sql');
// folder・archive コンテンツのショット用フィクスチャ。type=archive も同じツリーを
// 索引先にする(英検/年度でdirLevel軸2種、級でfilenameWord軸、共通テストで
// 「未設定」フォールバックを実演できるため → shots.ts)。
//
// seed.sql に folder を入れられないのは、実在する絶対パスが要るため。置き場を target/
// 配下にしてあるのは、片付け漏れが普段見ない場所 (macOS の `/var/folders/...`) に
// 溜まらないようにするため。この置き場自身を `roots` に入れてから登録する
// (→ docs/folders.md「公開できるフォルダ」)。
const FIXTURE_ROOT_DIR = path.join(REPO_ROOT, 'target/spec-fixtures');
const FIXTURE_DIR = path.join(FIXTURE_ROOT_DIR, '教材');
/** `FIXTURE_ROOT_DIR` を登録するときの名前。ピッカーやコンテンツの場所に、フルパスの代わりに出る。 */
const FIXTURE_ROOT_NAME = '共有';
// ファイル数がそのまま確認ダイアログの表示になるので、桁が揃って読みやすい数にする。
const FIXTURE_TREE: Record<string, string[]> = {
	'英検/2024': ['1級_リスニング.mp3', '1級_解答.pdf', '2級_リスニング.mp3'],
	'英検/2023': ['1級_リスニング.mp3', '1級_解答.pdf'],
	共通テスト: ['2024_リスニング.mp3', '2024_問題.pdf']
};
// 画像のプレビュー (→ docs/ui.md「画像のプレビュー」) のショット用。空のファイルでは縮小画像も
// ビューアも出ないので、中身のある PNG をここで組み立てる。横長と縦長を混ぜ、
// 行では正方形に切り抜き、ビューアでは縦横比を保って開くことを見せる。
type Rgb = [number, number, number];
const FIXTURE_IMAGES: Record<string, { width: number; height: number; from: Rgb; to: Rgb }> = {
	'写真/運動会_入場.png': { width: 1200, height: 800, from: [70, 130, 180], to: [240, 200, 120] },
	'写真/運動会_閉会式.png': { width: 800, height: 1200, from: [60, 120, 90], to: [230, 230, 210] }
};

/** 上端の `from` から下端の `to` へ色が変わる PNG (8bit RGB)。 */
function gradientPng(width: number, height: number, from: Rgb, to: Rgb): Buffer {
	const rowBytes = 1 + width * 3; // 行頭のフィルタ種別 (0 = なし) + 画素
	const pixels = Buffer.alloc(rowBytes * height);
	for (let y = 0; y < height; y++) {
		const t = y / (height - 1);
		const color = from.map((start, i) => Math.round(start + (to[i] - start) * t));
		for (let x = 0; x < width; x++) {
			pixels.set(color, y * rowBytes + 1 + x * 3);
		}
	}
	const chunk = (type: string, data: Buffer) => {
		const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
		const length = Buffer.alloc(4);
		length.writeUInt32BE(data.length);
		const crc = Buffer.alloc(4);
		crc.writeUInt32BE(crc32(body));
		return Buffer.concat([length, body, crc]);
	};
	const header = Buffer.alloc(13);
	header.writeUInt32BE(width, 0);
	header.writeUInt32BE(height, 4);
	header.set([8, 2, 0, 0, 0], 8); // 8bit・RGB・圧縮/フィルタ/インターレースは既定
	return Buffer.concat([
		Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
		chunk('IHDR', header),
		chunk('IDAT', deflateSync(pixels)),
		chunk('IEND', Buffer.alloc(0))
	]);
}

// フォルダ閲覧画面は更新日時を表示するため、実行時刻のままだと生成物が毎回差分を
// 持ってしまう。固定日時にして再現性を持たせる。
/** 中身のある PDF。ほかの PDF は空で、ビューアでは読めない案内になる。 */
const FIXTURE_PDFS: Record<string, string[]> = {
	'共通テスト/2024_問題.pdf': [
		'2024 Listening Test',
		'Part A  Questions 1 - 6',
		'Part B  Questions 7 - 10'
	]
};

/** 文字を並べただけの1ページの PDF。フォントは埋め込まず、標準の Helvetica を名前で指す。 */
function simplePdf(lines: string[]): Buffer {
	const text = lines
		.map((line, i) => `BT /F1 ${i === 0 ? 28 : 16} Tf 72 ${760 - i * 48} Td (${line}) Tj ET`)
		.join('\n');
	const objects = [
		'<< /Type /Catalog /Pages 2 0 R >>',
		'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
		'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>',
		`<< /Length ${Buffer.byteLength(text)} >>\nstream\n${text}\nendstream`,
		'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>'
	];
	let body = '%PDF-1.4\n';
	const offsets: number[] = [];
	objects.forEach((object, i) => {
		offsets.push(Buffer.byteLength(body));
		body += `${i + 1} 0 obj\n${object}\nendobj\n`;
	});
	const xref = Buffer.byteLength(body);
	body += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
	body += offsets.map((offset) => `${String(offset).padStart(10, '0')} 00000 n \n`).join('');
	body += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
	return Buffer.from(body);
}

const FIXTURE_MTIME = new Date('2024-04-01T00:00:00Z');

/** `dir` 配下の全ファイル・ディレクトリの更新日時を固定する(再帰)。 */
function setFixedMtimes(dir: string, mtime: Date): void {
	for (const entry of readdirSync(dir, { withFileTypes: true })) {
		const entryPath = path.join(dir, entry.name);
		if (entry.isDirectory()) setFixedMtimes(entryPath, mtime);
		utimesSync(entryPath, mtime, mtime);
	}
	utimesSync(dir, mtime, mtime);
}
// 原稿のフォルダの *.md をそれぞれ docs/generated/spec/<同名> に生成する。
// mo はフォルダ内の複数Markdownをサイドバー切り替えで表示するビューアのため、
// 単一の巨大ファイルではなくページ単位で分割している。
// 原稿はリポジトリの外に置くので、場所は WEBLAV_SPEC_DIR で受け取る。
// 相対パスは、just のレシピが動くリポジトリの直下から見る。
// 撮影を終えてから気づかないよう、読み込みの時点でフォルダがあるかを確かめる。
const SPEC_SRC_DIR = path.resolve(REPO_ROOT, process.env.WEBLAV_SPEC_DIR ?? '');
if (
	!process.env.WEBLAV_SPEC_DIR ||
	!statSync(SPEC_SRC_DIR, { throwIfNoEntry: false })?.isDirectory()
) {
	throw new Error('WEBLAV_SPEC_DIR に仕様書の原稿のフォルダを指定してください');
}
const OUT_DIR = path.join(REPO_ROOT, 'docs/generated/spec');
const ASSETS_DIR = path.join(OUT_DIR, 'assets');

/** SELECTを1本実行し、結果の1行目の `id` 列を返す。 */
function selectId(db: DatabaseSync, sql: string): number {
	const row = db.prepare(sql).get();
	if (row === undefined) throw new Error(`no row: ${sql}`);
	return Number(row.id);
}

/** シードとフィクスチャの行をDBに直接入れ、APIで作るフィクスチャに渡すidを返す。 */
function seedDatabase(dbPath: string): { groupId: number; folderId: number } {
	return withDatabase(dbPath, (db) => {
		db.exec(readFileSync(SEED_SQL_PATH, 'utf8'));

		// group(seed.sqlで作成済み)のidを引く。archive/fileをその子として登録するため
		// (→ 原稿の 05-groups.md のショット用、shots.ts参照)。
		const groupId = selectId(db, "SELECT id FROM contents WHERE title = '教材(グループ)'");

		// 公開できるフォルダ (→ docs/folders.md「公開できるフォルダ」)。フィクスチャの木の親を登録する。
		// folder の行と同じく絶対パスが環境依存なので、seed.sql ではなくここで入れる。
		db.prepare('INSERT INTO roots (name, path) VALUES (?, ?)').run(
			FIXTURE_ROOT_NAME,
			FIXTURE_ROOT_DIR
		);

		// folder の行だけは絶対パスが環境依存になるため、seed.sql ではなくここで入れる。
		// ルート直下のまま(既存のhome_with_contents等のショットが前提にしている並びを
		// 変えないため)。group側のfolderカードは別行(下記)で用意する。
		db.prepare(
			'INSERT INTO contents (type, title, path, description, visibility, created_by) VALUES ' +
				"('folder', '教材アーカイブ', ?, " +
				"'ディレクトリ選択UIのスクリーンショット用のフィクスチャです。', 'public', " +
				"(SELECT id FROM users WHERE username = 'admin'))"
		).run(FIXTURE_DIR);
		const folderId = selectId(db, "SELECT id FROM contents WHERE title = '教材アーカイブ'");

		// 原稿の 05-groups.md の folder カード用。既存の「教材アーカイブ」とは別行にする
		// (ルート直下の並びに影響を与えないため)。同じFIXTURE_DIRを指す。
		db.prepare(
			'INSERT INTO contents (type, parent_id, title, path, description, visibility, created_by) VALUES ' +
				"('folder', ?, '英検アーカイブ', ?, " +
				"'グループ内のfolderカードのスクリーンショット用のフィクスチャです。', 'authenticated', " +
				"(SELECT id FROM users WHERE username = 'admin'))"
		).run(groupId, FIXTURE_DIR);

		return { groupId, folderId };
	});
}

/** `05〜07`のショット用に、`type=archive`/`type=file`のフィクスチャをHTTP API経由で作る。
 *
 * DBへの直接INSERTにしないのは、軸の導出・表示タイトルの組み立てといった
 * ロジックを本物のAPI(rescan・軸CRUD・公開切り替え)にそのまま通すことで、
 * 実装が変わってもフィクスチャが自動的に追従するようにするため(手書きのSQLだと
 * スキーマや導出ロジックの変更に追従し忘れて乖離しうる)。
 */
async function setupArchiveFixtures(
	baseURL: string,
	groupId: number
): Promise<{ archiveId: number; fileId: number }> {
	const loginRes = await fetch(`${baseURL}/api/v1/auth/login`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ username: TEST_ADMIN.username, password: TEST_ADMIN.password })
	});
	if (!loginRes.ok) {
		throw new Error(`フィクスチャ用のログインに失敗しました: ${loginRes.status}`);
	}
	// node-fetchはクッキーを自動で持ち回らないため、Set-Cookieを自前で引き回す。
	const cookie = loginRes.headers
		.getSetCookie()
		.map((c) => c.split(';')[0])
		.join('; ');

	async function api(apiPath: string, init: RequestInit = {}): Promise<Response> {
		const res = await fetch(`${baseURL}/api/v1${apiPath}`, {
			...init,
			headers: { ...init.headers, Cookie: cookie }
		});
		if (!res.ok) {
			throw new Error(
				`${init.method ?? 'GET'} ${apiPath} が失敗しました(${res.status}): ${await res.text()}`
			);
		}
		return res;
	}

	async function apiJson(apiPath: string, method: string, body: unknown): Promise<Response> {
		return api(apiPath, {
			method,
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify(body)
		});
	}

	const ARCHIVE_TITLE = '英検・共通テスト索引';
	const archive = (await (
		await apiJson('/contents', 'POST', {
			type: 'archive',
			parentId: groupId,
			title: ARCHIVE_TITLE,
			path: FIXTURE_DIR,
			description: '軸による絞り込み・表示タイトルのスクリーンショット用のフィクスチャです。',
			visibility: 'authenticated',
			extensions: 'mp3,pdf'
		})
	).json()) as { id: number };
	const archiveId = archive.id;

	async function createAxis(
		name: string,
		source: 'dirLevel' | 'filenameWord',
		dirLevel: number | undefined,
		position: number
	): Promise<{ id: number }> {
		return (await (
			await apiJson(`/contents/${archiveId}/axes`, 'POST', { name, source, dirLevel, position })
		).json()) as { id: number };
	}

	// 種別(英検/共通テスト)・年度(2024/2023、共通テストは階層が無く「未設定」)の
	// 2つのdirLevel軸と、級(1級/2級、共通テストのファイル名には現れないため
	// これも「未設定」)のfilenameWord軸。年度・級どちらも共通テスト側は未設定になり、
	// 下記titleTemplateのフォールバック(→ docs/archive.md「表示タイトル」)を実演できる。
	await createAxis('種別', 'dirLevel', 1, 0);
	await createAxis('年度', 'dirLevel', 2, 1);
	const levelAxis = await createAxis('級', 'filenameWord', undefined, 2);

	await apiJson(`/contents/${archiveId}/axes/${levelAxis.id}/values`, 'PUT', {
		values: [{ rawValue: '1級' }, { rawValue: '2級' }]
	});

	// テンプレートは軸が揃ってから設定する(作成時点では軸が無く422になるため)。
	// 完全上書き型PUTなので、作成時のレスポンスをそのまま送り直す。
	await apiJson(`/contents/${archiveId}`, 'PUT', {
		...archive,
		titleTemplate: '{年度}年度 {種別} {級}'
	});

	await api(`/contents/${archiveId}/rescan`, { method: 'POST' });

	const items = (await (await api(`/contents/${archiveId}/items`)).json()) as Array<{
		id: number;
	}>;
	await apiJson(`/contents/${archiveId}/items`, 'PUT', {
		itemIds: items.map((item) => item.id),
		published: true
	});

	// type=file: multipart専用エンドポイント(→ src/api/contents.rs)で作る。
	const form = new FormData();
	form.set('title', '行事予定.pdf');
	form.set('parentId', String(groupId));
	form.set('visibility', 'authenticated');
	form.set(
		'file',
		new Blob([Buffer.from('%PDF-1.4\n%%EOF\n')], { type: 'application/pdf' }),
		'行事予定.pdf'
	);
	const file = (await (await api('/contents/upload', { method: 'POST', body: form })).json()) as {
		id: number;
	};

	return { archiveId, fileId: file.id };
}

async function main(): Promise<void> {
	// ショットが辿る先を作ってから起動する (一覧APIは実ファイルシステムを見るため)。
	rmSync(FIXTURE_DIR, { recursive: true, force: true });
	for (const [dir, files] of Object.entries(FIXTURE_TREE)) {
		const target = path.join(FIXTURE_DIR, dir);
		mkdirSync(target, { recursive: true });
		for (const file of files) writeFileSync(path.join(target, file), '');
	}
	for (const [file, { width, height, from, to }] of Object.entries(FIXTURE_IMAGES)) {
		const target = path.join(FIXTURE_DIR, file);
		mkdirSync(path.dirname(target), { recursive: true });
		writeFileSync(target, gradientPng(width, height, from, to));
	}
	for (const [file, lines] of Object.entries(FIXTURE_PDFS)) {
		writeFileSync(path.join(FIXTURE_DIR, file), simplePdf(lines));
	}
	setFixedMtimes(FIXTURE_DIR, FIXTURE_MTIME);

	// 起動に失敗したら、startBackend がログを出して片付けてから投げる。
	const backend = await startBackend('weblav-spec-');
	const { baseURL, dbPath } = backend;

	let browser: Browser | undefined;
	const cleanup = async () => {
		await browser?.close();
		await backend.stop();
	};
	process.on('SIGINT', () => {
		cleanup().finally(() => process.exit(130));
	});

	try {
		// startBackend は migrate の完了 (=DBファイル作成) を待って返すので、すぐシードを入れられる。
		const { groupId, folderId } = seedDatabase(dbPath);

		const { archiveId, fileId } = await setupArchiveFixtures(baseURL, groupId);
		const fixtures = {
			groupId,
			folderId,
			archiveId,
			fileId,
			sharedFolder: FIXTURE_ROOT_DIR,
			sharedFolderName: FIXTURE_ROOT_NAME
		};

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
			// localeを固定するのは、cookieが無ければ表示言語がブラウザの言語で決まり
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
