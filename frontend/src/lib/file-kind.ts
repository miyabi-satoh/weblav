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
export type ViewerFileKind = 'pdf' | 'video' | 'image' | 'office' | 'text';

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
