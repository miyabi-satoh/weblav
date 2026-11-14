/**
 * パスにクエリを足す。
 *
 * エスケープの仕方と、クエリが空のときに `?` を付けるかどうかを1か所に閉じるためのもの。
 * 値は `URLSearchParams` で符号化する (空白は `+` になる)。
 * **渡された項目はそのまま載せる**。値が空の項目を落としたいときは、呼び出し側で入れないこと
 * (空の値に意味がある画面があるため。→ アーカイブの絞り込み)。
 *
 * `resolve()` は `resolve('/folders/[id]?path=x', params)` の形でクエリも受け取れるが、
 * リテラルの `?` を書く必要があり、クエリの有無で呼び分けることになる。
 * クエリは base path の解決に関わらないので、解決済みのパスに後から足しても結果は変わらない。
 */
export function withQuery(base: string, params: Record<string, string>): string {
	// `URLSearchParams` は読み取り専用 (`toString()` のみ) で使い、mutate するメソッドは
	// 呼ばない (`svelte/prefer-svelte-reactivity` が `.set`/`.delete` 呼び出しを検出するため)。
	const query = new URLSearchParams(params).toString();
	return query === '' ? base : `${base}?${query}`;
}

/** `location.search` の形のクエリを、`withQuery` に渡せる形にする。同じキーが2つあれば後のもの。 */
export function queryParams(search: string): Record<string, string> {
	return Object.fromEntries(new URLSearchParams(search));
}

/** `pathname` と `search` (今のブラウザーの URL) の `name` だけを `value` に替えた href。`null` なら外す。ほかのクエリは残す。 */
export function withQueryParam(
	pathname: string,
	search: string,
	name: string,
	value: string | null
): string {
	const params = queryParams(search);
	delete params[name];
	return withQuery(pathname, value === null ? params : { ...params, [name]: value });
}

/**
 * 画面の様子を、止まってから URL に写すまでの間 (一覧の絞り込みの語・開いているもの)。
 * Safari は短い間に何度も履歴を書き換えると例外を投げるため、打つたび (IME の変換中も入力が届く)・
 * 移るたび (左右キーを押し続けると続けて移る) には写さない。
 */
export const URL_WRITE_DELAY_MS = 300;
