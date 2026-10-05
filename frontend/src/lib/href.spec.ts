import { describe, expect, it } from 'vitest';
import { withQuery } from './href';

describe('withQuery', () => {
	it('項目が無ければ ? を付けない', () => {
		expect(withQuery('/folders/1', {})).toBe('/folders/1');
	});

	it('値が空の項目もそのまま載せる', () => {
		// 「その軸は空で絞る」を書いた共有リンクを、別の軸を操作した拍子に落とさないため。
		expect(withQuery('/archives/1', { year: '2024', round: '' })).toBe(
			'/archives/1?year=2024&round='
		);
	});

	it('記号と空白をエスケープする', () => {
		// 空白は `+`、`/` や `&` は %エンコード。いずれも URLSearchParams で読み戻せる。
		const href = withQuery('/folders/1', { path: 'a b/c&d' });
		expect(href).toBe('/folders/1?path=a+b%2Fc%26d');
		expect(new URL(href, 'http://x').searchParams.get('path')).toBe('a b/c&d');
	});

	it('複数の項目を & でつなぐ', () => {
		expect(withQuery('/', { sort: 'new', lang: 'en' })).toBe('/?sort=new&lang=en');
	});
});
