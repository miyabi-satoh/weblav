import { withQuery } from '$lib/href';
import * as m from '$lib/paraglide/messages.js';

/** アーカイブの一覧の並び順 (→ docs/archive.md「エンドポイント一覧」, docs/ui.md「ホーム・グループ・フォルダ・アーカイブの並び順」)。 */
export type ArchiveSort = 'axis' | 'title' | 'new';

const DEFAULT_ARCHIVE_SORT: ArchiveSort = 'axis';

/** ドロップダウンに出す順。`label` は表示言語の切り替えに追従させるため関数のまま持つ。 */
export const ARCHIVE_SORT_OPTIONS: { value: ArchiveSort; label: () => string }[] = [
	{ value: 'axis', label: m.archive_sort_axis },
	{ value: 'title', label: m.browse_sort_title },
	{ value: 'new', label: m.browse_sort_new }
];

/**
 * URL クエリの値を並び順として読む。知らない値は既定として扱う
 * (サーバー側と同じ扱い。共有されたリンクを壊さないため → docs/archive.md「エンドポイント一覧」)。
 */
export function parseArchiveSort(value: string | null): ArchiveSort {
	if (value === 'title' || value === 'new') {
		return value;
	}
	return DEFAULT_ARCHIVE_SORT;
}

/**
 * 並び順を付けた URL を組み立てる。既定のときはクエリを付けない。
 * `filters` は軸の絞り込みなど、`sort` 以外の既存クエリ (→ +page.svelte の `data.filters`)。
 * `base` は `resolve()` の戻り値を渡すこと (呼び出し側で静的に解決させるため)。
 */
export function archiveSortHref(
	base: string,
	filters: Record<string, string>,
	sort: ArchiveSort
): string {
	const next: Record<string, string> = { ...filters };
	if (sort === DEFAULT_ARCHIVE_SORT) {
		delete next.sort;
	} else {
		next.sort = sort;
	}
	return withQuery(base, next);
}
