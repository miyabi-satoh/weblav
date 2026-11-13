import { describe, expect, it } from 'vitest';
import type { ViewerFile } from './file-viewer.svelte';
import type { ViewerImage } from './image-viewer';
import { findViewerItem, viewerItems } from './viewer-items';

type Entry = { name: string; image?: boolean; viewable?: boolean };

const toImage = (entry: Entry): ViewerImage | undefined =>
	entry.image
		? { src: entry.name, thumbnailSrc: entry.name, width: 1, height: 1, title: entry.name }
		: undefined;

const toFile = (entry: Entry): ViewerFile | undefined =>
	entry.viewable
		? { src: entry.name, title: entry.name, fileName: entry.name, kind: 'pdf' }
		: undefined;

describe('viewerItems', () => {
	it('keeps images and files in the list order, skipping rows the viewers do not open', () => {
		const items = viewerItems<Entry>(
			[
				{ name: 'a.jpg', image: true },
				{ name: 'b.pdf', viewable: true },
				{ name: 'c.mp3' },
				{ name: 'd.png', image: true, viewable: true }
			],
			toImage,
			toFile,
			(entry) => entry.name
		);
		expect(
			items.map((item) => [item.type, item.type === 'image' ? item.image.src : item.file.src])
		).toEqual([
			['image', 'a.jpg'],
			['file', 'b.pdf'],
			// 画像としても開ける行は、行の出し分けと同じく画像にする。
			['image', 'd.png']
		]);
	});
});

describe('findViewerItem', () => {
	const items = viewerItems<Entry>(
		[
			{ name: 'a.jpg', image: true },
			{ name: 'b.pdf', viewable: true }
		],
		toImage,
		toFile,
		(entry) => `key-${entry.name}`
	);

	it('finds the item by the key the list gave it, with the whole list for previous and next', () => {
		expect(findViewerItem(items, 'key-b.pdf')).toEqual({ items, index: 1 });
	});

	it('finds nothing for a key the list does not have (the file was deleted or filtered out)', () => {
		expect(findViewerItem(items, 'key-c.pdf')).toBeUndefined();
	});
});
