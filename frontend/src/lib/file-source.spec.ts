import { describe, expect, it } from 'vitest';
import { sourceDownloadHref, sourceKey, sourceThumbnailHref } from './file-source';

describe('file source hrefs', () => {
	it('points a folder content row at the content download and thumbnail', () => {
		const source = { kind: 'content', id: 3 } as const;
		expect(sourceDownloadHref(source, 'a b/c.pdf')).toBe(
			'/api/v1/contents/3/download?path=a+b%2Fc.pdf'
		);
		expect(sourceThumbnailHref(source, 'x.jpg')).toBe('/api/v1/contents/3/thumbnail?path=x.jpg');
	});

	it('points a share row at the share download and thumbnail', () => {
		const source = { kind: 'share', id: 7 } as const;
		expect(sourceDownloadHref(source, 'docs/a.pdf')).toBe(
			'/api/v1/shares/7/download?path=docs%2Fa.pdf'
		);
		expect(sourceThumbnailHref(source, 'x.jpg')).toBe('/api/v1/shares/7/thumbnail?path=x.jpg');
	});

	it('keeps rows of a content and a share with the same id apart', () => {
		expect(sourceKey({ kind: 'content', id: 1 })).not.toBe(sourceKey({ kind: 'share', id: 1 }));
	});
});
