import type FolderIcon from '@lucide/svelte/icons/folder';

/** 場所の階層の1段。押すと、その段の画面を開く。 */
export type RowLocationLevel = { label: string; href: string; icon: typeof FolderIcon };

/**
 * 検索の結果の行に出す、どこにあるか (→ docs/search.md「画面」)。
 * `levels` は上から順で、最後が行のすぐ上 (親)。
 */
export type RowLocation = {
	levels: RowLocationLevel[];
	/**
	 * 行に出し始める段。範囲の中を探しているとき、範囲より上は行ごとに同じなので出さない。
	 * 押して出る一覧には、上の段も全部出す。
	 */
	from?: number;
};

const SEPARATOR = ' / ';

/**
 * 行に出す文字。`front` は親より上、`tail` は親で、幅が足りないときは `front` の側だけを切る。
 * 同じ名前の行は親で見分けるので、親は残す。区切りは `tail` の頭に付け、`front` が切れても残るようにする。
 */
export function rowLocationText(location: RowLocation): {
	front: string;
	tail: string;
	/** 省かない全部の段。読み上げの名前に使う。 */
	full: string;
} {
	const labels = location.levels.map((level) => level.label);
	// 範囲の段そのものは残す。範囲のすぐ下の行でも、場所が空にならないようにするため。
	const shown = labels.slice(Math.min(location.from ?? 0, labels.length - 1));
	const front = shown.slice(0, -1).join(SEPARATOR);
	const last = shown[shown.length - 1] ?? '';
	return {
		front,
		tail: front === '' ? last : `${SEPARATOR}${last}`,
		full: labels.join(SEPARATOR)
	};
}

/** 検索の範囲にしている場所。`depth` は、フォルダーの中の階層を範囲にしているときの、その深さ。 */
export type ScopePlace = { contentId: number; depth: number };

/**
 * 範囲の中を探しているときの、行に出し始める段 (`RowLocation.from`)。範囲の外の行と、範囲が無いときは `undefined`。
 * 場所は「上のグループ → 当たりを持つコンテンツ (`ownId`) → その中の階層」の順に並んでいる前提。
 * 範囲がグループならそのグループから、フォルダーの階層ならその階層から出す。
 */
export function scopeStart(
	scope: ScopePlace | null,
	ancestorIds: number[],
	ownId?: number
): number | undefined {
	if (!scope) return undefined;
	const group = ancestorIds.indexOf(scope.contentId);
	if (group >= 0) return group;
	return ownId === scope.contentId ? ancestorIds.length + scope.depth : undefined;
}
