// 画面を撮るスクリプト (`just spec` の generate-spec.ts・`just flip-videos` の flip-videos.ts) が共有する、
// シードとフィクスチャ (公開できるフォルダーの中のファイル・コンテンツ・アーカイブ)。

import type { DatabaseSync } from 'node:sqlite';
import { rmSync, mkdirSync, readFileSync, writeFileSync, readdirSync, utimesSync } from 'node:fs';
import path from 'node:path';
import { crc32, deflateSync } from 'node:zlib';
import { withDatabase, type Backend } from './backend-process.ts';
import type { ShotFixtures } from './shots.ts';
import { TEST_ADMIN } from './test-account.ts';

const REPO_ROOT = path.resolve(import.meta.dirname, '../..');
const SEED_SQL_PATH = path.join(import.meta.dirname, 'seed.sql');
// folder コンテンツのショット用フィクスチャ。一覧の表示を使う folder も同じツリーを
// 索引先にする(英検/年度でdirLevel軸2種、級でfilenameWord軸、共通テストで
// 「未設定」フォールバックを実演できるため → shots.ts)。
//
// seed.sql に folder を入れられないのは、実在する絶対パスが要るため。置き場を target/
// 配下にしてあるのは、片付け漏れが普段見ない場所 (macOS の `/var/folders/...`) に
// 溜まらないようにするため。この置き場自身を `roots` に入れてから登録する
// (→ docs/folders.md「公開できるフォルダー」)。
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
// ビューアーも出ないので、中身のある PNG をここで組み立てる。横長と縦長を混ぜ、
// 行では正方形に切り抜き、ビューアーでは縦横比を保って開くことを見せる。
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

// フォルダー閲覧画面は更新日時を表示するため、実行時刻のままだと生成物が毎回差分を
// 持ってしまう。固定日時にして再現性を持たせる。
/** 中身のある PDF。ほかの PDF は空で、ビューアーでは読めない案内になる。 */
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
/** ショットが辿る先のファイルを作る。一覧 API は実ファイルシステムを見るので、backend を起動する前に呼ぶ。 */
export function writeFixtureFiles(): void {
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
}
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

		// 公開できるフォルダー (→ docs/folders.md「公開できるフォルダー」)。フィクスチャの木の親を登録する。
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

/** `05〜07`のショット用に、一覧の表示を使う folder と `type=file` のフィクスチャをHTTP API経由で作る。
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
	// フォルダーを作ってから、一覧の表示を使い、新しいファイルを公開しない設定にする (→ docs/folders.md「2つの表示」)。
	const created = (await (
		await apiJson('/contents', 'POST', {
			type: 'folder',
			parentId: groupId,
			title: ARCHIVE_TITLE,
			path: FIXTURE_DIR,
			description: '分類による絞り込み・表示タイトルのスクリーンショット用のフィクスチャです。',
			visibility: 'authenticated'
		})
	).json()) as { id: number };
	const archive = (await (
		await apiJson(`/contents/${created.id}`, 'PUT', {
			...created,
			listView: true,
			newFilesPublic: false,
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

	const { snapshot } = (await (await api(`/contents/${archiveId}/manage-archive`)).json()) as {
		snapshot: string;
	};
	await apiJson(`/contents/${archiveId}/items`, 'PUT', { snapshot, published: true });

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

/** 起動した backend にシードとフィクスチャを入れ、ショットに渡す id を返す。先に `writeFixtureFiles` を呼んでおく。 */
export async function setUpFixtures(backend: Backend): Promise<ShotFixtures> {
	// startBackend は migrate の完了 (=DBファイル作成) を待って返すので、すぐシードを入れられる。
	const { groupId, folderId } = seedDatabase(backend.dbPath);
	const { archiveId, fileId } = await setupArchiveFixtures(backend.baseURL, groupId);
	return {
		groupId,
		folderId,
		archiveId,
		fileId,
		sharedFolder: FIXTURE_ROOT_DIR,
		sharedFolderName: FIXTURE_ROOT_NAME
	};
}
