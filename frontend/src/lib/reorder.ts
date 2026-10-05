/**
 * `from` の要素を取り出して `to` の位置に入れた新しい配列を返す。元の配列は変えない。
 * `from` が範囲外なら並びを変えない。`to` は範囲内に収める。
 */
export function moveItem<T>(items: readonly T[], from: number, to: number): T[] {
	const result = [...items];
	if (from < 0 || from >= result.length) return result;
	const [item] = result.splice(from, 1);
	result.splice(Math.max(0, Math.min(to, result.length)), 0, item);
	return result;
}
