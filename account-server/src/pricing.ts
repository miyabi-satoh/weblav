// 料金のページ (site/src/pages/pricing.astro) を、売っていない間だけ書き換える (→ docs/pro.md「売り方」)。
// 料金のページは置いてある静的なページで、売っているかどうか (Stripe の設定があるか) は窓口しか知らない。
// 申し込むボタンを残すと、押した人をサインインさせてから「いまは買えません」と断ることになる。

/** 料金のページの申し込むボタンを外し、ページに仕込んである「準備中」の表示を出す。 */
export function closeSale(page: Response): Response {
	const closed = new HTMLRewriter()
		.on('a[data-plan]', { element: (a) => void a.remove() })
		.on('[data-sale-closed]', { element: (note) => void note.removeAttribute('hidden') })
		.transform(page);
	// 置いてあるページと中身が違うので、同じ ETag を付けない。売り始めた後に、書き換えた写しを出し続けさせないため。
	const headers = new Headers(closed.headers);
	headers.delete('etag');
	headers.set('cache-control', 'no-store');
	return new Response(closed.body, { status: closed.status, headers });
}
