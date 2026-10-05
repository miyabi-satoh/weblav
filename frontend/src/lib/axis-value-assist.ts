// 値の辞書で、照合語を決める手がかり (語の候補・保存前の結果) を出すための判定 (→ docs/archive.md「軸の設定を助ける表示」)。
import type { components } from '$lib/api/schema';

type AxisSource = components['schemas']['AxisSource'];
type FilenameWord = components['schemas']['FilenameWord'];

/** 語の候補を一度に出す数。「さらに表示する」を押すたびに、この数ずつ増やす。 */
export const WORD_CANDIDATES_PAGE_SIZE = 10;

/** 照合語の比べ方。サーバーの照合と同じく、前後の空白と英字の大文字小文字を無視する。 */
function wordKey(word: string): string {
	return word.trim().toLowerCase();
}

/**
 * 語の候補から、照合語の表に今ある語 (保存前の行も含む) を除く。
 * 検索語が空なら、既定で隠す語も除く。検索語があれば、それを含む語を隠す語も含めて出す
 * (大文字小文字は区別しない)。並びはサーバーの順 (件数の多い順) のまま。
 */
export function wordCandidates(
	words: readonly FilenameWord[],
	rawValues: readonly string[],
	query: string
): FilenameWord[] {
	const inTable = new Set(rawValues.map(wordKey).filter((key) => key !== ''));
	const needle = wordKey(query);
	return words.filter(
		(word) =>
			(needle === '' ? !word.hiddenByDefault : wordKey(word.word).includes(needle)) &&
			!inTable.has(wordKey(word.word))
	);
}

/** ファイル名の語の軸の、照合語の数の上限。サーバーと揃える (→ docs/archive.md「軸の設定を助ける表示」)。 */
export const FILENAME_WORD_VALUES_LIMIT = 1_000;

/**
 * 保存前の結果を問い合わせられない理由。問い合わせてよければ `null`。
 * 空の行・重複した行・語の軸での上限超えは、サーバーが保存と同じく 422 を返す。
 * 重複の判定はサーバーに合わせて、前後の空白を落とし、大文字小文字は区別する。
 */
export function axisValuesPreviewBlocker(
	rawValues: readonly string[],
	source: AxisSource
): 'incomplete' | 'tooMany' | null {
	if (source === 'filenameWord' && rawValues.length > FILENAME_WORD_VALUES_LIMIT) return 'tooMany';
	const seen = new Set<string>();
	for (const rawValue of rawValues) {
		const value = rawValue.trim();
		if (value === '' || seen.has(value)) return 'incomplete';
		seen.add(value);
	}
	return null;
}
