/** 整数の入力欄に打った文字列を読んだ結果。 */
export type IntegerInputParse =
	{ kind: 'empty' } | { kind: 'ok'; value: number } | { kind: 'invalid' };

// 数字だけを受ける。符号・小数・指数は、上限のある正の整数の設定には要らないため。
const DIGITS = /^[0-9]+$/;

/**
 * 前後の空白を除いて読み、`min`〜`max` の整数なら値を返す。
 * 全角の数字も受ける。日本語入力をオンにしたまま打つと全角になるため。
 */
export function parseIntegerInput(text: string, min: number, max: number): IntegerInputParse {
	const trimmed = text.normalize('NFKC').trim();
	if (trimmed === '') return { kind: 'empty' };
	if (!DIGITS.test(trimmed)) return { kind: 'invalid' };
	const value = Number(trimmed);
	return value >= min && value <= max ? { kind: 'ok', value } : { kind: 'invalid' };
}
