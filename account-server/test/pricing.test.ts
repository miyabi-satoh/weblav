import { describe, expect, it } from 'vitest';
import { closeSale } from '../src/pricing';

describe('closeSale', () => {
	it('removes the buy buttons and shows the note that sales have not started', async () => {
		const page = new Response(
			'<ul><li><a class="buy" data-plan href="/account/buy?plan=year">年額で申し込む</a>' +
				'<p data-sale-closed hidden>申し込みは準備中です</p></li></ul><a href="/terms/">利用規約</a>',
			{ headers: { 'content-type': 'text/html; charset=utf-8', etag: '"abc"' } }
		);
		const closed = closeSale(page);
		const html = await closed.text();
		expect(html).not.toContain('/account/buy');
		expect(html).toContain('<p data-sale-closed>申し込みは準備中です</p>');
		expect(html).toContain('<a href="/terms/">利用規約</a>');
		expect(closed.headers.get('etag')).toBeNull();
		expect(closed.headers.get('cache-control')).toBe('no-store');
	});
});
