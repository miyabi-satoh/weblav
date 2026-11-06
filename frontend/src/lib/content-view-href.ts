import { resolve } from '$app/paths';
import type { components } from '$lib/api/schema';
import { isLinksFileName } from '$lib/file-kind';
import { linksFileHref } from '$lib/links-file';

type Content = Pick<
	components['schemas']['AdminContentResponse'],
	'id' | 'type' | 'parentId' | 'url' | 'fileName' | 'listView' | 'visibility' | 'createdBy'
>;

/** 閲覧画面での開き先。`external` なら外のサイトなので、閲覧画面と同じく別のタブで開く。 */
export type ContentViewHref = { href: string; external: boolean };

function parentHref(content: Content): string {
	return content.parentId == null
		? resolve('/')
		: resolve('/groups/[id]', { id: String(content.parentId) });
}

/**
 * 管理画面から、そのコンテンツを閲覧画面で開く先 (→ docs/ui.md「UI 全般」)。
 * 閲覧側での見え方を確かめるため、そのコンテンツが並ぶ一覧 (入っているグループ、ルート直下ならホーム) を開く。
 * 操作している人の一覧に並ばないもの (「非表示」・他人の「作成者のみ」) は、上を開いても見えないので、それ自体を開く。
 *
 * @param userId 操作しているユーザー。
 */
export function contentViewHref(content: Content, userId: number | undefined): ContentViewHref {
	const listed =
		content.visibility !== 'hidden' &&
		(content.visibility !== 'private' || content.createdBy === userId);
	return listed ? { href: parentHref(content), external: false } : contentSelfHref(content);
}

/** そのコンテンツ自体の開き先。閲覧画面の一覧が行ごとに開く先と揃える (→ content-list.svelte)。 */
export function contentSelfHref(content: Content): ContentViewHref {
	const id = String(content.id);
	switch (content.type) {
		case 'group':
			return { href: resolve('/groups/[id]', { id }), external: false };
		// 一覧の表示を使うフォルダーは、閲覧画面と同じくそちらを開く (→ docs/folders.md「2つの表示」)。
		case 'folder':
			return {
				href: content.listView
					? resolve('/archives/[id]', { id })
					: resolve('/folders/[id]', { id }),
				external: false
			};
		case 'link':
			return { href: content.url ?? resolve('/'), external: content.url != null };
		case 'file':
		case 'shortcut':
			if (content.fileName && isLinksFileName(content.fileName)) {
				return { href: linksFileHref(content.id), external: false };
			}
			// 専用のページが無いファイル (画像・PDF・音声・動画) は、「非表示」などでも、入っているグループを開く。
			return { href: parentHref(content), external: false };
	}
}
