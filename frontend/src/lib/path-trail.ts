/**
 * ディレクトリ選択UIのパンくず用に、絶対パスを階層へ分解する。
 *
 * 各要素の `path` は元の文字列の前方一致部分をそのまま切り出したもので、区切り文字を
 * 自前で連結しない。OS ごとの区切りの差をクライアントに持ち込まないため
 * (→ docs/folders.md「一覧 API」)。
 */

export interface PathCrumb {
	/** 画面に出す名前。ルート部分は `C:\` や `\\server\share` のように区切りを含む。 */
	label: string;
	/** この階層を開くために一覧APIへ渡す絶対パス。 */
	path: string;
}

/**
 * Windows形式のパスか。ドライブ文字始まりか UNC (`\\`) 始まりで判定する。
 * Unix系では `\` はディレクトリ名に使える普通の文字なので、区切りとして扱うのは
 * Windows 形式と分かった場合だけに限る。
 */
function isWindowsPath(full: string): boolean {
	return /^[A-Za-z]:[\\/]/.test(full) || full.startsWith('\\\\');
}

/**
 * `\\` の直後 (`start`) から、サーバー名と共有名を1つのまとまりとして数える。
 * UNC は共有名まで揃って初めてパスとして成立する。
 */
function uncRootLength(full: string, start: number): number {
	const serverEnd = full.indexOf('\\', start);
	if (serverEnd === -1) return full.length;
	const shareEnd = full.indexOf('\\', serverEnd + 1);
	return shareEnd === -1 ? full.length : shareEnd;
}

/**
 * パスの先頭にある「1つのまとまりとして扱うルート部分」の長さ。
 *
 * 分割してはいけない単位が4種類ある。`C:\` から `C:` を切り出すとドライブ相対パスに
 * なって意味が変わり、UNC は共有名まで要る。Unix系の `/` は区切りそのものがルート。
 *
 * verbatim 形式 (`\\?\C:\`・`\\?\UNC\server\share`) を先に判定するのは、
 * バックエンドが返すのが `std::fs::canonicalize` 済みのパスであり、Windows では
 * これらの形になるため。`\\` 始まりの判定を先に通すと `\\?\C:` のように壊れる。
 *
 * `//server/share` のような区切りを `/` にした UNC は扱わない。バックエンドが返すのは
 * canonicalize 済みのパスだけで、Rust はこの形を返さない。POSIX でも `//` は正当な
 * パスなので、UNC と決めつける方が危うい。
 */
function rootLength(full: string, windows: boolean): number {
	if (!windows) return full.startsWith('/') ? 1 : 0;
	// `\\?\UNC\` は8文字。
	if (/^\\\\\?\\UNC\\/i.test(full)) return uncRootLength(full, 8);
	// `\\?\C:\` は7文字。
	if (/^\\\\\?\\[A-Za-z]:\\/.test(full)) return 7;
	if (full.startsWith('\\\\')) return uncRootLength(full, 2);
	// `C:\` のようなドライブルート。区切りまで含める。
	return 3;
}

/**
 * 絶対パスをパンくず用に分解する。空文字列 (上位の一覧) では現在位置が無いため、空の配列を返す。
 *
 * `root` を渡すと、そこから先だけを返す。コンテンツの登録先を選ぶ一覧は「公開できる
 * フォルダー」の外を辿れないので、押しても 422 になる上位を出さない (→ docs/folders.md「一覧 API」)。
 */
export function pathTrail(full: string, root?: string | null): PathCrumb[] {
	if (full === '') return [];
	const trail = splitPath(full);
	if (!root) return trail;
	// 見つからないときは切らない。パンくずが消えるより、押せない階層が残る方が軽い。
	const start = trail.findIndex((crumb) => isSamePath(crumb.path, root));
	return start === -1 ? trail : trail.slice(start);
}

/**
 * パンくずの階層のパスが、一覧APIの返した起点 (`root`) と同じ位置か。
 * `pathTrail` が起点を探すときと、起点を名前に置き換えるときで規則を揃える。
 */
export function isSamePath(a: string, b: string): boolean {
	return a.toLowerCase() === b.toLowerCase();
}

function splitPath(full: string): PathCrumb[] {
	const windows = isWindowsPath(full);
	const isSeparator = (ch: string) => ch === '/' || (windows && ch === '\\');

	const trail: PathCrumb[] = [];
	const rootEnd = rootLength(full, windows);
	if (rootEnd > 0) {
		const root = full.slice(0, rootEnd);
		trail.push({ label: root, path: root });
	}

	let start = rootEnd;
	for (let i = rootEnd; i <= full.length; i += 1) {
		if (i !== full.length && !isSeparator(full[i])) continue;
		const label = full.slice(start, i);
		if (label !== '') trail.push({ label, path: full.slice(0, i) });
		start = i + 1;
	}
	return trail;
}
