/** ファイル名の拡張子 (小文字、ドットを除く)。ドットが無ければ `undefined`。 */
export function fileExtension(fileName: string): string | undefined {
	const dot = fileName.lastIndexOf('.');
	return dot < 0 ? undefined : fileName.slice(dot + 1).toLowerCase();
}

/**
 * ファイル名の拡張子から、ページ内で再生すべき音声ファイルかを判定する。
 *
 * 振り分けはファイルの種類(拡張子→MIME)で決まり、コンテンツの種類には依存しない
 * (→ docs/ui.md「音声のページ内プレイヤー」)。バックエンドの実際のMIME判定(`mime_guess`crate)とは
 * 独立した簡易判定で、ズレても影響は「新規タブで開くか、ページ内再生するか」という
 * 表示上の振り分けにとどまる(配信自体は同じダウンロードエンドポイントを使う)。
 */
const AUDIO_EXTENSIONS = new Set(['mp3', 'm4a', 'wav', 'ogg', 'oga', 'flac', 'aac', 'wma', 'opus']);

export function isAudioFileName(fileName: string): boolean {
	const extension = fileExtension(fileName);
	return extension !== undefined && AUDIO_EXTENSIONS.has(extension);
}

/**
 * ページ内のビューアで開く、音声と PhotoSwipe で開く画像のほかのファイル
 * (→ docs/ui.md「PDF・動画・テキストのビューア」)。
 */
export type ViewerFileKind = 'pdf' | 'video' | 'image' | 'office' | 'text' | 'embed';

// 動画と画像は、表示できるかがブラウザによる形式も入れる (mov・HEIC など)。表示できなければ、
// ビューアが「読めなかったとき」の案内を出す。wmv・avi のように、どのブラウザでも表示できない形式は入れない。
// 画像は、サーバーが大きさを読めず PhotoSwipe で開けないものだけ (→ docs/ui.md「画像のプレビュー」)。
// 音声とこの一覧の拡張子は、サーバーの `src/api/text_files.rs` (`KIND_BY_EXTENSION`) と揃える。
// サーバーは、ここで種類の決まるファイルを `isText` にしない。
const VIEWER_EXTENSIONS: Record<string, ViewerFileKind> = {
	pdf: 'pdf',
	mp4: 'video',
	m4v: 'video',
	webm: 'video',
	mov: 'video',
	heic: 'image',
	heif: 'image',
	avif: 'image',
	tif: 'image',
	tiff: 'image',
	docx: 'office',
	xlsx: 'office',
	pptx: 'office'
};

/** リンクの一覧のファイル (→ docs/ui.md「リンクの一覧のファイル」)。サーバーの `LINKS_FILE_SUFFIX` と揃える。 */
const LINKS_FILE_SUFFIX = '.links.toml';

export function isLinksFileName(fileName: string): boolean {
	return fileName.toLowerCase().endsWith(LINKS_FILE_SUFFIX);
}

/**
 * `isText` は、サーバーが中身を見てテキストと判定したか (一覧の API の `isText`)。
 * リンクの一覧のファイルはテキストでも、ビューアでなく一覧の画面で開く。
 */
export function viewerFileKind(fileName: string, isText: boolean): ViewerFileKind | undefined {
	if (isLinksFileName(fileName)) return undefined;
	const extension = fileExtension(fileName);
	const kind = extension === undefined ? undefined : VIEWER_EXTENSIONS[extension];
	return kind ?? (isText ? 'text' : undefined);
}

/**
 * URL のファイルで、ビューアの画像として開く拡張子。サーバーの `thumbnails` の `IMAGE_EXTENSIONS` と揃える。
 * URL のファイルは大きさを読まないので、PhotoSwipe でなくビューアで開く (→ docs/ui.md「URL のファイル」)。
 */
const REMOTE_IMAGE_EXTENSIONS = new Set(['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp']);

export type RemoteFileKind = ViewerFileKind | 'audio';

/** URL のホスト名。ページのタイトルが取れないリンクの題にする。読めない URL はそのまま返す。 */
export function urlHost(url: string): string {
	try {
		return new URL(url).host;
	} catch {
		return url;
	}
}

/** URL のパスの最後の部分を戻したファイル名。http・https でない・読めない URL は `undefined`。 */
export function remoteFileName(url: string): string | undefined {
	let parsed: URL;
	try {
		parsed = new URL(url);
	} catch {
		return undefined;
	}
	if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return undefined;
	const last = parsed.pathname.split('/').pop() ?? '';
	try {
		return decodeURIComponent(last);
	} catch {
		return undefined;
	}
}

/**
 * LAN の相手を指す URL か (IP が公開アドレスでない・`.local`・ドットの無い名前)。サーバーは公開アドレスにだけ
 * つなぐので、中継しない。名前を引いた先が LAN のものはここでは見分けられず、ビューアの読めなかったときの
 * 案内から、元の URL を新しいタブで開く。
 */
export function isLanUrl(url: string): boolean {
	let host: string;
	try {
		host = new URL(url).hostname.toLowerCase();
	} catch {
		return false;
	}
	if (host.startsWith('[')) {
		const v6 = host.slice(1, -1);
		return v6 === '::1' || /^f[cd]/.test(v6) || /^fe[89ab]/.test(v6);
	}
	const v4 = host.split('.').map(Number);
	if (v4.length === 4 && v4.every((part) => Number.isInteger(part) && part >= 0 && part <= 255)) {
		const [a, b] = v4;
		return (
			a === 0 ||
			a === 10 ||
			a === 127 ||
			(a === 100 && b >= 64 && b <= 127) ||
			(a === 169 && b === 254) ||
			(a === 172 && b >= 16 && b <= 31) ||
			(a === 192 && b === 168)
		);
	}
	return !host.includes('.') || host.endsWith('.local');
}

/**
 * `link` コンテンツの URL を、サーバーの中継を通してプレイヤー・ビューアで開くなら、その種類
 * (→ docs/ui.md「URL のファイル」)。拡張子で開き方の決まるファイルだけで、サーバーの
 * `remote_file::relayed_file_name` と揃える。LAN の URL と、それ以外のページは `undefined` (新しいタブで開く)。
 */
export function remoteFileKind(url: string): RemoteFileKind | undefined {
	if (isLanUrl(url)) return undefined;
	const fileName = remoteFileName(url);
	if (fileName === undefined) return undefined;
	if (isAudioFileName(fileName)) return 'audio';
	const extension = fileExtension(fileName);
	if (extension === undefined) return undefined;
	if (REMOTE_IMAGE_EXTENSIONS.has(extension)) return 'image';
	return VIEWER_EXTENSIONS[extension];
}
