/**
 * API エンドポイントへの直リンクを組み立てる。
 *
 * SvelteKit のルートではないため `resolve()` は通さない。`resolve()` は route id を
 * 解決するもので、`/api/v1/...` は対象外。
 *
 * このリンクを置く `<a>` には `rel="external"` を付ける。付けないと SPA ルーターが
 * アプリ内の遷移として横取りする。
 */

import { withQuery } from '$lib/href';

/**
 * `GET /api/v1/contents/{id}/download` へのリンク。
 *
 * `path` は folder コンテンツ配下の相対パス。file コンテンツでは省く。
 */
export function contentDownloadHref(contentId: number, path?: string): string {
	const base = `/api/v1/contents/${contentId}/download`;
	return withQuery(base, path === undefined || path === '' ? {} : { path });
}

/**
 * `GET /api/v1/admin/logs` へのリンク。サーバーのログを、古い日から順につなげたテキストの添付で返す。
 * `download` 属性を付けた `<a>` は SPA ルーターが横取りしないので、`rel="external"` は要らない。
 */
export function adminLogsHref(): string {
	return '/api/v1/admin/logs';
}

/** `GET /api/v1/admin/backup` へのリンク。作りながら返す zip。ファイル名は画面が `download` 属性で付ける。 */
export function adminBackupHref(): string {
	return '/api/v1/admin/backup';
}

/** `GET /api/v1/contents/{id}/remote` へのリンク。`link` コンテンツの URL のファイルを、サーバーが取ってきて流す。 */
export function contentRemoteHref(contentId: number): string {
	return `/api/v1/contents/${contentId}/remote`;
}

/** `GET /api/v1/contents/{id}/thumbnail` へのリンク。`path` は `contentDownloadHref` と同じ。 */
export function contentThumbnailHref(contentId: number, path?: string): string {
	const base = `/api/v1/contents/${contentId}/thumbnail`;
	return withQuery(base, path === undefined || path === '' ? {} : { path });
}

/** `GET /api/v1/contents/{id}/items/{item_id}/download` へのリンク。 */
export function archiveItemDownloadHref(contentId: number, itemId: number): string {
	return `/api/v1/contents/${contentId}/items/${itemId}/download`;
}

/** `GET /api/v1/contents/{id}/items/{item_id}/manage-download` へのリンク。管理画面用で、未公開も開ける。 */
export function archiveItemManageDownloadHref(contentId: number, itemId: number): string {
	return `/api/v1/contents/${contentId}/items/${itemId}/manage-download`;
}

/** `GET /api/v1/contents/{id}/items/{item_id}/thumbnail` へのリンク。 */
export function archiveItemThumbnailHref(contentId: number, itemId: number): string {
	return `/api/v1/contents/${contentId}/items/${itemId}/thumbnail`;
}

/** `GET /api/v1/help/images/{name}` へのリンク。マニュアルの本文と同じ言語 (`locale`) の画像を取る。 */
export function helpImageHref(name: string, locale: string): string {
	return withQuery(`/api/v1/help/images/${name}`, { locale });
}
