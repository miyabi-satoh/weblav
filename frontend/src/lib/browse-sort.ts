import { withQuery } from '$lib/href';
import * as m from '$lib/paraglide/messages.js';

/** ホームとグループの一覧の並び順 (→ docs/ui.md「ホーム・グループ・フォルダ・アーカイブの並び順」)。 */
export type BrowseSort = 'title' | 'new';

export const DEFAULT_BROWSE_SORT: BrowseSort = 'title';

/** ドロップダウンに出す順。`label` は表示言語の切り替えに追従させるため関数のまま持つ。 */
export const SORT_OPTIONS: { value: BrowseSort; label: () => string }[] = [
	{ value: 'title', label: m.browse_sort_title },
	{ value: 'new', label: m.browse_sort_new }
];

/**
 * URL クエリの値を並び順として読む。知らない値は既定として扱う
 * (サーバー側と同じ扱い。共有されたリンクを壊さないため → docs/ui.md「ホーム・グループ・フォルダ・アーカイブの並び順」)。
 */
export function parseBrowseSort(value: string | null): BrowseSort {
	return value === 'new' ? 'new' : DEFAULT_BROWSE_SORT;
}

/**
 * 並び順を付けた URL を組み立てる。既定のときはクエリを付けない。
 * `base` は `resolve()` の戻り値を渡すこと (呼び出し側で静的に解決させるため)。
 * `rest` は並び順を変えても残すクエリ (ページ内の絞り込みの語など)。
 */
export function browseSortHref(
	base: string,
	sort: BrowseSort,
	rest: Record<string, string> = {}
): string {
	return withQuery(base, { ...(sort === DEFAULT_BROWSE_SORT ? {} : { sort }), ...rest });
}
