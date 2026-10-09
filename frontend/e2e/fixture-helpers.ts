// フォルダー・アーカイブの e2e 用に、実在するディレクトリを用意する。
//
// サーバーが同じマシンで動いている前提で、リポジトリの target/ 配下にファイルの木を作り、
// その絶対パスを API で登録する。木の置き場そのものを「公開できるフォルダー」に登録して
// おかないと、コンテンツ側の登録が通らない (→ auth.setup.ts、docs/folders.md「公開できるフォルダー」)。
// サーバーからパスが見えない (別のマシンで動いている等) ときは、登録が 422 になるので
// テストを飛ばす。
import { existsSync, mkdirSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { test, type APIRequestContext } from '@playwright/test';
import { removeContentByApi, send, type Visibility } from './api-helpers';
import { uniqueId } from './helpers';
import { SERVER_IS_REMOTE } from './server-location';

const FIXTURE_BASE = path.resolve(import.meta.dirname, '../../target/e2e-fixtures');

/** コンテンツの登録先に使う木の置き場。ここ自身を「公開できるフォルダー」に登録する。 */
const CONTENT_ROOT = path.join(FIXTURE_BASE, 'content');
/**
 * `CONTENT_ROOT` を登録するときの名前。フォルダー名 (`content`) のままだと、開発用 DB に
 * 同じ名前のフォルダーが登録されていたときに、名前の重なりで断られる。
 */
const CONTENT_ROOT_NAME = 'e2e-content';

/**
 * 「公開できるフォルダー」の登録そのものを試す木の置き場。`CONTENT_ROOT` の外に置く
 * (登録済みのフォルダーと範囲が重なるものは登録できないため)。
 */
const ROOT_FIXTURE_BASE = path.join(FIXTURE_BASE, 'roots');

/**
 * 年度のフォルダーの下に、音声と PDF を置いた小さな木。ファイル名の語 (listening / answer) で
 * 種類の軸を、第1階層で年度の軸を作れる。中身は再生・表示できなくてよい (画面の動きだけを見る)。
 */
const FIXTURE_FILES: Record<string, string> = {
	'2024/listening.mp3': 'ID3',
	'2024/answer.pdf': '%PDF-1.4\n%%EOF\n',
	'2023/listening.mp3': 'ID3'
};

/**
 * `name` のディレクトリにファイルの木を作り、その絶対パスを返す。
 * 途中で失敗したら作りかけを消してから投げ直す (呼び出し側の finally には root が届かないため)。
 */
export function createFixtureTree(
	name: string,
	files: Record<string, string> = FIXTURE_FILES
): string {
	return createTreeIn(CONTENT_ROOT, name, files);
}

/** 「公開できるフォルダー」として登録するための木。登録済みのフォルダーの外に作る。 */
export function createRootFixtureTree(name: string): string {
	return createTreeIn(ROOT_FIXTURE_BASE, name, FIXTURE_FILES);
}

function createTreeIn(base: string, name: string, files: Record<string, string>): string {
	const root = path.join(base, name);
	try {
		for (const [relPath, body] of Object.entries(files)) {
			const filePath = path.join(root, relPath);
			mkdirSync(path.dirname(filePath), { recursive: true });
			writeFileSync(filePath, body);
		}
	} catch (err) {
		removeFixtureTree(root);
		throw err;
	}
	return root;
}

/** 後片付け専用。テスト本体の失敗を覆い隠さないよう、ここでの失敗は警告に留める。 */
export function removeFixtureTree(root: string): void {
	try {
		rmSync(root, { recursive: true, force: true });
	} catch (err) {
		console.warn(`e2e のフィクスチャの後片付けに失敗しました (${root}):`, err);
	}
}

type DirectoryContent = Record<string, unknown> & { id: number };

/** サーバーからパスが見えないときの 422 のメッセージ (→ src/api/fs.rs)。 */
const PATH_NOT_ACCESSIBLE = 'path does not exist or is not accessible';

/** 既に登録済みのときの 422 の内訳 (→ src/api/error_detail.rs)。 */
const ROOT_ALREADY_REGISTERED = '"kind":"rootAlreadyRegistered"';

/**
 * コンテンツの木の置き場を「公開できるフォルダー」に登録する (→ auth.setup.ts)。
 *
 * 開発用サーバーに当てる `just e2e` では前回の登録が残っているので、二重登録の 422 は
 * 登録済みとみなす。サーバーからパスが見えないときも通す (個々のテストが飛ばす)。
 * 別のマシンのサーバーでは登録の口が通らないので、試さない。
 */
export async function registerContentRoot(request: APIRequestContext): Promise<void> {
	if (SERVER_IS_REMOTE) return;
	mkdirSync(CONTENT_ROOT, { recursive: true });
	const res = await request.post('/api/v1/admin/roots', {
		data: { path: CONTENT_ROOT, name: CONTENT_ROOT_NAME }
	});
	if (res.ok()) return;
	const text = await res.text();
	if (
		res.status() === 422 &&
		(text.includes(ROOT_ALREADY_REGISTERED) || text.includes(PATH_NOT_ACCESSIBLE))
	) {
		return;
	}
	throw new Error(`公開できるフォルダーの登録に失敗しました (${res.status()}): ${text}`);
}

/**
 * folder / archive を登録する。サーバーからパスが見えないときだけ `null` を返す
 * (呼び出し側でテストを飛ばす)。登録できない場所などほかの 422 は、判定が壊れたのを
 * 見逃さないよう例外にする。
 */
async function createDirectoryContentByApi(
	request: APIRequestContext,
	body: {
		type: 'folder' | 'archive';
		title: string;
		path: string;
		extensions?: string;
		visibility?: Visibility;
	}
): Promise<DirectoryContent | null> {
	// 置き場を登録していないので、どのパスもルートの外として断られる。
	if (SERVER_IS_REMOTE) return null;
	const res = await request.post('/api/v1/contents', { data: body });
	if (!res.ok()) {
		const text = await res.text();
		if (res.status() === 422 && text.includes(PATH_NOT_ACCESSIBLE)) return null;
		throw new Error(`コンテンツの登録に失敗しました (${res.status()}): ${text}`);
	}
	return (await res.json()) as DirectoryContent;
}

export const FIXTURE_UNREACHABLE =
	'サーバーから e2e のフィクスチャが見えない (同じマシンで動いていない)';

/** 登録先を選ぶ一覧で、コンテンツの木の置き場へ辿るための位置 (→ `sharedContentRoot`)。 */
export type SharedContentRoot = {
	/** 上位の一覧に並ぶ、置き場を含む登録済みのフォルダー。置き場そのものとは限らない。 */
	root: string;
	/** `root` の名前。一覧とパンくずには、フルパスではなくこれが出る。 */
	rootName: string;
	/** 置き場の実体パス。 */
	contentRoot: string;
};

/**
 * 登録先を選ぶ一覧の上位から、コンテンツの木の置き場へ辿れるか。辿れなければ `null`。
 *
 * 上位に並ぶのは登録済みのフォルダーだけで、置き場そのものとは限らない。開発用 DB で置き場を
 * 含む親 (リポジトリなど) が先に登録されていると、置き場の登録は重なりで断られる
 * (→ `registerContentRoot`) が、その親から辿れる。
 * サーバーは canonicalize した形で返すので、こちらも実体パスにして比べる。
 */
export async function sharedContentRoot(
	request: APIRequestContext
): Promise<SharedContentRoot | null> {
	// 手元のフィクスチャは相手のマシンに無いので、パスが重なっても辿れない。
	if (SERVER_IS_REMOTE) return null;
	if (!existsSync(CONTENT_ROOT)) return null;
	const contentRoot = realpathSync(CONTENT_ROOT);
	const res = await request.get('/api/v1/admin/fs/dirs', { params: { path: '' } });
	if (!res.ok()) return null;
	const { entries } = (await res.json()) as { entries: { name: string; path: string }[] };
	const root = entries.find((entry) => {
		const relative = path.relative(entry.path, contentRoot);
		return !relative.startsWith('..') && !path.isAbsolute(relative);
	});
	return root === undefined ? null : { root: root.path, rootName: root.name, contentRoot };
}

/** 登録側の画面で、階層の名前を並べる区切り (`$lib/root-location` の `LOCATION_SEPARATOR`)。 */
export const LOCATION_SEPARATOR = ' / ';

/**
 * 置き場の中の `dirName` が、登録側の画面でどう出るか。フルパスではなく
 * 「公開できるフォルダーの名前 / その先」になる (→ docs/folders.md「公開できるフォルダー」)。
 */
export function sharedLocationLabel(shared: SharedContentRoot, dirName: string): string {
	const target = path.join(shared.contentRoot, dirName);
	return [shared.rootName, ...path.relative(shared.root, target).split(path.sep)].join(
		LOCATION_SEPARATOR
	);
}

/**
 * フィクスチャの木を作って folder / archive として登録し、`body` に渡す。
 * サーバーからパスが見えなければテストを飛ばす。登録したコンテンツと木は、途中で失敗しても片付ける。
 * タイトルは `namePrefix` から作る一意な名前 (木のディレクトリ名と同じ)。archive は木の mp3・pdf を拾う。
 * `files` (パス → 中身) を渡すと、既定の木の代わりにその木を作る。
 */
export async function withDirectoryContent(
	request: APIRequestContext,
	options: { type: 'folder' | 'archive'; namePrefix: string; files?: Record<string, string> },
	body: (content: DirectoryContent, name: string) => Promise<void>
): Promise<void> {
	const name = uniqueId(options.namePrefix);
	const root = createFixtureTree(name, options.files);
	let contentId: number | undefined;
	try {
		const content = await createDirectoryContentByApi(request, {
			type: options.type,
			title: name,
			path: root,
			extensions: options.type === 'archive' ? 'mp3,pdf' : undefined
		});
		test.skip(content === null, FIXTURE_UNREACHABLE);
		contentId = content!.id;
		await body(content!, name);
	} finally {
		if (contentId !== undefined) await removeContentByApi(request, contentId);
		removeFixtureTree(root);
	}
}

type ArchiveItem = { id: number; relPath: string; published: boolean; title: string };

/** アーカイブを再スキャンし、索引したアイテムの一覧を返す。 */
export async function rescanArchive(
	request: APIRequestContext,
	archiveId: number
): Promise<ArchiveItem[]> {
	await send(request, 'post', `/contents/${archiveId}/rescan`);
	return (await send(request, 'get', `/contents/${archiveId}/items`)) as ArchiveItem[];
}

/** `relPaths` のアイテムだけを公開する。 */
export async function publishItems(
	request: APIRequestContext,
	archiveId: number,
	items: ArchiveItem[],
	relPaths: string[]
): Promise<void> {
	await send(request, 'put', `/contents/${archiveId}/items`, {
		itemIds: items.filter((item) => relPaths.includes(item.relPath)).map((item) => item.id),
		published: true
	});
}

/**
 * 閲覧画面のテスト用に、年度 (第1階層)・種類 (ファイル名の語) の2軸とテンプレートを設定し、
 * 全アイテムを公開する。表示タイトルは「2024 リスニング」の形になる。
 * 種類は、`kindFilterable` を渡さなければ絞り込みに出さない。
 */
export async function setUpArchiveForBrowsing(
	request: APIRequestContext,
	archive: DirectoryContent,
	options: { kindFilterable?: boolean } = {}
): Promise<void> {
	const id = archive.id;
	await send(request, 'post', `/contents/${id}/axes`, {
		name: '年度',
		source: 'dirLevel',
		dirLevel: 1,
		position: 0
	});
	const kind = (await send(request, 'post', `/contents/${id}/axes`, {
		name: '種類',
		source: 'filenameWord',
		position: 1,
		filterable: options.kindFilterable ?? false
	})) as { id: number };
	await send(request, 'put', `/contents/${id}/axes/${kind.id}/values`, {
		values: [
			{ rawValue: 'listening', displayName: 'リスニング' },
			{ rawValue: 'answer', displayName: '解答' }
		]
	});
	// 完全上書き型の PUT なので、作成時の応答をそのまま送り直す。
	await send(request, 'put', `/contents/${id}`, { ...archive, titleTemplate: '{年度} {種類}' });
	const items = await rescanArchive(request, id);
	await publishItems(
		request,
		id,
		items,
		items.map((item) => item.relPath)
	);
}
