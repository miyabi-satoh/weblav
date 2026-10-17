import { describe, expect, it } from 'vitest';
import { closeSale } from '../src/sale';

describe('closeSale', () => {
	it('removes the buy button and shows the note that sales have not started', async () => {
		const page = new Response(
			'<section><a class="buy" data-buy href="/account/buy?lang=ja">クレジットを購入</a>' +
				'<p data-sale-closed hidden>準備中</p></section><a href="/terms/">利用規約</a>',
			{ headers: { 'content-type': 'text/html; charset=utf-8', etag: '"abc"' } }
		);
		const closed = closeSale(page);
		const html = await closed.text();
		expect(html).not.toContain('/account/buy');
		expect(html).toContain('<p data-sale-closed>準備中</p>');
		expect(html).toContain('<a href="/terms/">利用規約</a>');
		expect(closed.headers.get('etag')).toBeNull();
		expect(closed.headers.get('cache-control')).toBe('no-store');
	});
});
