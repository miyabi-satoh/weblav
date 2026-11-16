import { createRawSnippet } from 'svelte';
import { describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { ViewerImage } from '#lib/image-viewer.ts';
import ListRowImageLink from './list-row-image-link.svelte';

const { openViewerItem } = vi.hoisted(() => ({
	openViewerItem: vi.fn<(...args: unknown[]) => Promise<void>>(() => new Promise(() => {}))
}));
vi.mock('#lib/viewer-items.ts', () => ({ openViewerItem }));

const children = createRawSnippet(() => ({ render: () => '<span>A</span>' }));

function imageOf(src: string): ViewerImage {
	return { src, thumbnailSrc: src, width: 1, height: 1, title: src };
}

describe('ListRowImageLink', () => {
	it('keeps opening when the list is rebuilt with the same image (a link card was refreshed)', async () => {
		const screen = await render(ListRowImageLink, {
			image: imageOf('/a.jpg'),
			items: [{ type: 'image', image: imageOf('/a.jpg') }],
			children
		});
		await userEvent.click(page.getByRole('link', { name: 'A' }));
		const signal = openViewerItem.mock.calls.at(-1)?.[2] as AbortSignal;

		await screen.rerender({
			image: imageOf('/a.jpg'),
			items: [{ type: 'image', image: imageOf('/a.jpg') }]
		});
		expect(signal.aborted).toBe(false);

		// 行が別の画像に使い回されたら、前の画像は開かない。
		await screen.rerender({ image: imageOf('/b.jpg') });
		expect(signal.aborted).toBe(true);
	});
});
