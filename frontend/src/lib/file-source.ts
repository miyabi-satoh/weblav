import {
	contentDownloadHref,
	contentThumbnailHref,
	shareDownloadHref,
	shareThumbnailHref
} from '$lib/api/urls';

/**
 * フォルダーの中のファイル・ディレクトリの行が、どこのものか。
 * フォルダーのコンテンツ (`/folders/[id]`・検索) と共有 (`/shares/[id]`) で、同じ一覧の部品を使うため。
 * `path` はどちらも、その入れ物の一番上からの相対パス。
 */
export type FileSource = { kind: 'content'; id: number } | { kind: 'share'; id: number };

/** ファイルを開く・ダウンロードする API への直リンク (→ $lib/api/urls.ts)。 */
export function sourceDownloadHref(source: FileSource, path: string): string {
	return source.kind === 'content'
		? contentDownloadHref(source.id, path)
		: shareDownloadHref(source.id, path);
}

/** 縮小画像の API への直リンク。 */
export function sourceThumbnailHref(source: FileSource, path: string): string {
	return source.kind === 'content'
		? contentThumbnailHref(source.id, path)
		: shareThumbnailHref(source.id, path);
}

/** 一覧の行の key に使う、入れ物の見分け。 */
export function sourceKey(source: FileSource): string {
	return `${source.kind}:${source.id}`;
}
