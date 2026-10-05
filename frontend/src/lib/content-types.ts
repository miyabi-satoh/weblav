/**
 * コンテンツの種類ごとの扱いの違い。表示ラベルは `content-labels.ts`。
 */
import ArchiveIcon from '@lucide/svelte/icons/archive';
import FileIcon from '@lucide/svelte/icons/file';
import FolderIcon from '@lucide/svelte/icons/folder';
import FolderTreeIcon from '@lucide/svelte/icons/folder-tree';
import LinkIcon from '@lucide/svelte/icons/link';
import type { components } from '$lib/api/schema';

type ContentType = components['schemas']['ContentType'];

/**
 * サーバー上のディレクトリを登録する種類 (`folder` / `archive`) か。
 *
 * この種類だけがパスを持ち、登録には「公開できるフォルダ」が要る (→ docs/folders.md「公開できるフォルダ」)。
 */
export function isDirectoryContentType(type: ContentType): boolean {
	return type === 'folder' || type === 'archive';
}

/** 種類を表すアイコン。大きさや色は呼び出し側で決める。 */
export function contentTypeIcon(type: ContentType): typeof FileIcon {
	if (type === 'link') return LinkIcon;
	if (type === 'folder') return FolderIcon;
	if (type === 'archive') return ArchiveIcon;
	if (type === 'group') return FolderTreeIcon;
	return FileIcon;
}
