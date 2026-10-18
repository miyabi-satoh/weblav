/** 表示タイトルのテンプレートで軸を指す書き方。サーバーの `placeholder` と同じ形にする。 */
export function axisPlaceholder(name: string): string {
	return `{${name}}`;
}

// 文字・数字・プレースホルダーの括弧以外 (空白・区切りの記号) を、軸の間の区切りとみなす。
const LEADING_SEPARATORS = /^[^\p{L}\p{N}{}]+/u;
const TRAILING_SEPARATORS = /[^\p{L}\p{N}{}]+$/u;

/**
 * テンプレートから軸の書き方を外した形を返す。軸を削除するときに、削除した後のテンプレートの案として出す。
 * 外した所の前後に余る区切りは、前にほかの部品があれば前の側を、無ければ後ろの側を除く
 * (`{年度} {季節}` から年度を外すと `{季節}`、季節を外すと `{年度}`)。区切りが文字に付いている
 * テンプレートは直しきれないので、利用者がその場で直せるように出す。
 */
export function removeAxisFromTemplate(template: string, name: string): string {
	const placeholder = axisPlaceholder(name);
	let result = template;
	for (let at = result.indexOf(placeholder); at >= 0; at = result.indexOf(placeholder)) {
		let before = result.slice(0, at);
		let after = result.slice(at + placeholder.length);
		const trailing = before.match(TRAILING_SEPARATORS)?.[0] ?? '';
		if (trailing !== '' && before.length > trailing.length) {
			before = before.slice(0, -trailing.length);
		} else {
			after = after.replace(LEADING_SEPARATORS, '');
		}
		result = before + after;
	}
	return result.trim();
}
