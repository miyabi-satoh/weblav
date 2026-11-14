import { afterEach, describe, expect, it } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { closeImageViewer, openImageViewer } from './image-viewer.ts';

/** 横長 (200×100) の画像。回したら縦長に置き直されるかを、大きさで見る。 */
async function landscapeImage(): Promise<string> {
	const canvas = document.createElement('canvas');
	canvas.width = 200;
	canvas.height = 100;
	const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve));
	return URL.createObjectURL(blob!);
}

function shownImage(): HTMLImageElement {
	const image = document.querySelector<HTMLImageElement>(
		'.pswp__item:not([aria-hidden="true"]) img.pswp__img'
	);
	if (!image) throw new Error('the viewer shows no image');
	return image;
}

describe('openImageViewer', () => {
	afterEach(() => {
		closeImageViewer();
		document.querySelector('.pswp')?.remove();
	});

	it('rotate button turns the image upright in a portrait box', async () => {
		const src = await landscapeImage();
		await openImageViewer(
			[{ type: 'image', image: { src, thumbnailSrc: src, width: 200, height: 100, title: 'a' } }],
			0,
			new AbortController().signal,
			{ animate: false, onLeave: () => {} }
		);
		await expect.poll(() => shownImage().getBoundingClientRect().width).toBe(200);

		await userEvent.click(page.getByRole('button', { name: /^(回転|Rotate)$/ }));

		await expect.poll(() => shownImage().style.transform).toContain('rotate(90deg)');
		const box = shownImage().getBoundingClientRect();
		expect([box.width, box.height]).toEqual([100, 200]);
	});
});
