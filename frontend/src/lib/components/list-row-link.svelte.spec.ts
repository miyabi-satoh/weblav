import { afterEach, describe, expect, it } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import ListRowLink from './list-row-link.svelte';
import { linkDetail } from '$lib/link-detail.svelte';

describe('ListRowLink', () => {
	afterEach(() => {
		linkDetail.open = false;
	});

	it('opens the detail dialog instead of the page on a plain click', async () => {
		await render(ListRowLink, { href: 'https://example.com/a', title: 'Example' });
		await userEvent.click(page.getByRole('link', { name: /Example/ }));

		expect(linkDetail.open).toBe(true);
		expect(linkDetail.current?.href).toBe('https://example.com/a');
	});

	it('moves between the links of the list in the detail dialog', async () => {
		const items = [
			{ href: 'https://example.com/a', title: 'A' },
			{ href: 'https://example.com/b', title: 'B' }
		];
		await render(ListRowLink, {
			href: 'https://example.com/b',
			title: 'B',
			details: { items, index: 1 }
		});
		await userEvent.click(page.getByRole('link', { name: /B/ }));

		expect(linkDetail.current?.title).toBe('B');
		linkDetail.previous();
		expect(linkDetail.current?.title).toBe('A');
		expect(linkDetail.index).toBe(0);
	});

	it('leaves LAN links to open directly', async () => {
		await render(ListRowLink, { href: 'http://192.168.1.1/', title: 'Router' });
		const link = page.getByRole('link', { name: /Router/ });
		// 新しいタブが開かないよう、既定の動きだけを止めて、詳しい表示が開かないことを見る。
		link.element().addEventListener('click', (event) => event.preventDefault());
		await userEvent.click(link);

		expect(linkDetail.open).toBe(false);
	});
});
