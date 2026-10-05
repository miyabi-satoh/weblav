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
