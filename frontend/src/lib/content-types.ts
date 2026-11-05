/**
 * コンテンツの種類ごとの扱いの違い。表示ラベルは `content-labels.ts`。
 */
import FileIcon from '@lucide/svelte/icons/file';
import FolderIcon from '@lucide/svelte/icons/folder';
import FolderTreeIcon from '@lucide/svelte/icons/folder-tree';
import LinkIcon from '@lucide/svelte/icons/link';
import type { components } from '$lib/api/schema';

type ContentType = components['schemas']['ContentType'];

/**
 * サーバー上のディレクトリを登録する種類 (`folder`) か。
 *
 * この種類だけがパスを持ち、登録には「共有」が要る (→ docs/folders.md「共有」)。
 */
export function isDirectoryContentType(type: ContentType): boolean {
	return type === 'folder';
}

/** 共有の中の場所を指す種類か。場所を選ぶ欄を持つ。ファイルを指すもの (`shortcut`) は、数える確認とパターンを持たない。 */
export function isPathContentType(type: ContentType): boolean {
	return type === 'folder' || type === 'shortcut';
}

/** 種類を表すアイコン。大きさや色は呼び出し側で決める。 */
export function contentTypeIcon(type: ContentType): typeof FileIcon {
	if (type === 'link') return LinkIcon;
	if (type === 'folder') return FolderIcon;
	if (type === 'group') return FolderTreeIcon;
	return FileIcon;
}
