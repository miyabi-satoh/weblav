/**
 * 「パターンに追加...」の候補。名前の絞り込みの語から、見せるもののパターンを作る (→ docs/ui.md「パターンに追加」)。
 * 絞り込みの記法とパターンの記法は別なので、語の中の記号は記号としてでなく、その文字として当てる。
 */

/** パターンで特別な意味を持つ文字。`[ ]` で包むと、その文字として当たる (`file_filter` のテストで確かめている)。 */
const GLOB_SYMBOLS = /[*?[\]{}\\]/g;

function escapeGlob(term: string): string {
	return term.replace(GLOB_SYMBOLS, (symbol) => `[${symbol}]`);
}

/** 語の前後と間に `*` を置く。`jpg` なら `*jpg*`、`英語 2024` なら `*英語*2024*`。語が無ければ空。 */
export function patternCandidate(filterText: string): string {
	const terms = filterText.split(/\s+/).filter((term) => term !== '');
	return terms.length === 0 ? '' : `*${terms.map(escapeGlob).join('*')}*`;
}

/** 今のパターン (1行に1つ) の末尾に足す。同じものがあれば足さない。 */
export function appendPattern(existing: string | null | undefined, pattern: string): string {
	const lines = (existing ?? '')
		.split('\n')
		.map((line) => line.trim())
		.filter((line) => line !== '');
	if (!lines.includes(pattern)) lines.push(pattern);
	return lines.join('\n');
}
