/** 表示タイトルのテンプレートで軸を指す書き方。サーバーの `placeholder` と同じ形にする。 */
export function axisPlaceholder(name: string): string {
	return `{${name}}`;
}

// 軸を囲む括弧 (`[{教科}]`・`（{回}）`・`「{題}」` など)。囲む軸を外すときに一緒に外す。
// プレースホルダーの `{` `}` も同じ文字の種類なので除く。
const OPENING_BRACKET = /(?!\{)[\p{Ps}\p{Pi}]$/u;
const CLOSING_BRACKET = /^(?!\})[\p{Pe}\p{Pf}]/u;
// 文字・数字・括弧 (プレースホルダーの `{` `}` を含む) 以外 (空白・区切りの記号) を、軸の間の区切りとみなす。
// 括弧を区切りに含めると、前の軸を囲む閉じ括弧まで消えるため。
const LEADING_SEPARATORS = /^[^\p{L}\p{N}{}\p{Ps}\p{Pe}\p{Pi}\p{Pf}]+/u;
const TRAILING_SEPARATORS = /[^\p{L}\p{N}{}\p{Ps}\p{Pe}\p{Pi}\p{Pf}]+$/u;

/**
 * テンプレートから軸の書き方を外した形を返す。軸を削除するときに、削除した後のテンプレートの案として出す。
 * - 軸を囲む括弧は、重なっていても一緒に外す (`[{季節}] {場所}` から季節を外すと `{場所}`)。
 * - 外した所の前後に余る区切りは、前にほかの部品があれば前の側を、無ければ (区切りだけ・括弧の開きの直後も) 後ろの側を除く
 *   (`{年度} {季節}` から年度を外すと `{季節}`、季節を外すと `{年度}`)。
 * - 前の部品が区切りなしで付いていたときは、後ろの区切りを残す (`{年度}（{回}） {科目}` から回を外すと `{年度} {科目}`)。
 * - 区切りが文字に付いているテンプレートは直しきれないので、利用者がその場で直せるように出す。
 */
export function removeAxisFromTemplate(template: string, name: string): string {
	const placeholder = axisPlaceholder(name);
	let result = template;
	for (let at = result.indexOf(placeholder); at >= 0; at = result.indexOf(placeholder)) {
		let before = result.slice(0, at);
		let after = result.slice(at + placeholder.length);
		while (OPENING_BRACKET.test(before) && CLOSING_BRACKET.test(after)) {
			before = before.slice(0, -1);
			after = after.slice(1);
		}
		const trailing = before.match(TRAILING_SEPARATORS)?.[0] ?? '';
		if (trailing !== '' && before.length > trailing.length) {
			before = before.slice(0, -trailing.length);
		} else if (trailing.length === before.length || OPENING_BRACKET.test(before)) {
			after = after.replace(LEADING_SEPARATORS, '');
		}
		result = before + after;
	}
	return result.trim();
}
