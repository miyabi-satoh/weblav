import { describe, expect, it } from 'vitest';
import {
	archivePreviewKey,
	parseArchivePreviewKey,
	previewHref,
	wantedPreviewKey
} from './preview-query';

describe('wantedPreviewKey', () => {
	const url = new URL('http://localhost/folders/1?preview=a.pdf');

	it('reads the key from the URL when shallow routing has not written one (reload, shared link)', () => {
		expect(wantedPreviewKey({}, url)).toBe('a.pdf');
	});

	it('prefers the key in page.state, because shallow routing does not update page.url', () => {
		expect(wantedPreviewKey({ preview: 'b.pdf' }, url)).toBe('b.pdf');
	});

	it('treats null in page.state as closed, even if the URL the page loaded with still has the key', () => {
		expect(wantedPreviewKey({ preview: null }, url)).toBeNull();
	});

	it('is null when neither has a key', () => {
		expect(wantedPreviewKey({}, new URL('http://localhost/folders/1'))).toBeNull();
	});
});

describe('previewHref', () => {
	it('adds the key and keeps the other queries', () => {
		expect(previewHref('/folders/1', '?path=2024&sort=new', 'a b.pdf')).toBe(
			'/folders/1?path=2024&sort=new&preview=a+b.pdf'
		);
	});

	it('replaces the key that is already there', () => {
		expect(previewHref('/folders/1', '?preview=a.pdf&filter=x', 'b.pdf')).toBe(
			'/folders/1?filter=x&preview=b.pdf'
		);
	});

	it('removes the key, leaving no "?" when nothing else is left', () => {
		expect(previewHref('/folders/1', '?preview=a.pdf', null)).toBe('/folders/1');
	});
});

describe('archive preview keys', () => {
	it('round-trips the position and the item id', () => {
		expect(parseArchivePreviewKey(archivePreviewKey(1234, 56))).toEqual({
			index: 1234,
			itemId: 56
		});
	});

	it('rejects keys of another shape (a hand-edited or old URL)', () => {
		for (const key of ['', '12', '12-', '-3', '1-2-3', 'a-1', '1.5-2']) {
			expect(parseArchivePreviewKey(key), key).toBeUndefined();
		}
	});
});
