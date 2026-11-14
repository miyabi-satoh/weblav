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

	it('rotate button turns the image 90 degrees per press, keeping it centered', async () => {
		const src = await landscapeImage();
		await openImageViewer(
			[{ type: 'image', image: { src, thumbnailSrc: src, width: 200, height: 100, title: 'a' } }],
			0,
			new AbortController().signal,
			{ animate: false, onLeave: () => {} }
		);
		await expect.poll(() => shownImage().getBoundingClientRect().width).toBe(200);
		const center = (box: DOMRect) => [box.x + box.width / 2, box.y + box.height / 2];
		const start = center(shownImage().getBoundingClientRect());
		const button = page.getByRole('button', { name: /^(回転|Rotate)$/ });

		for (const [rotation, size] of [
			[90, [100, 200]],
			[180, [200, 100]],
			[270, [100, 200]]
		] as const) {
			await userEvent.click(button);
			await expect.poll(() => shownImage().style.transform).toContain(`rotate(${rotation}deg)`);
			const box = shownImage().getBoundingClientRect();
			expect([box.width, box.height]).toEqual(size);
			// 回した後も、PhotoSwipe が置いた枠 (中央) に収まっている。
			const [x, y] = center(box);
			expect(x).toBeCloseTo(start[0]);
			expect(y).toBeCloseTo(start[1]);
		}

		await userEvent.click(button);
		await expect.poll(() => shownImage().style.transform).toBe('');
	});
});
