import { describe, expect, it } from 'vitest';
import { collapseCjkLineBreaks, renderManual } from './manual-markdown';

describe('collapseCjkLineBreaks', () => {
	it('日本語どうしの改行だけを詰める', () => {
		expect(collapseCjkLineBreaks('です。\nログイン')).toBe('です。ログイン');
		expect(collapseCjkLineBreaks('ブラウザーから\n見られる')).toBe('ブラウザーから見られる');
	});

	it('連続する改行をすべて詰める', () => {
		expect(collapseCjkLineBreaks('あ\nい\nう')).toBe('あいう');
	});

	it('境目にタグが挟まっても詰める', () => {
		expect(collapseCjkLineBreaks('打ちます。\n<code>satoh</code> の部分')).toBe(
			'打ちます。<code>satoh</code> の部分'
		);
		expect(collapseCjkLineBreaks('採ります</strong>。\n<code>math</code> と')).toBe(
			'採ります</strong>。<code>math</code> と'
		);
		expect(collapseCjkLineBreaks('です</strong>\n次の')).toBe('です</strong>次の');
	});

	it('欧文が絡む改行は触らない', () => {
		expect(collapseCjkLineBreaks('the\nserver')).toBe('the\nserver');
		expect(collapseCjkLineBreaks('環境変数\nWEBLAV_HOME')).toBe('環境変数\nWEBLAV_HOME');
		expect(collapseCjkLineBreaks('WebLAV\nから')).toBe('WebLAV\nから');
	});

	it('pre の中は行をそのまま残す', () => {
		const html = '<p>あ\nい</p><pre><code>あ\nい</code></pre><p>う\nえ</p>';
		expect(collapseCjkLineBreaks(html)).toBe(
			'<p>あい</p><pre><code>あ\nい</code></pre><p>うえ</p>'
		);
	});
});

describe('renderManual', () => {
	it('表をキーボードで操作できる領域で包む', () => {
		const html = renderManual('| a | b |\n|---|---|\n| 1 | 2 |\n', '表', 'ja');
		expect(html).toContain('<div class="table-scroll" tabindex="0" role="region" aria-label="表">');
		expect(html).toContain('<table>');
		expect(html).toContain('</table>\n</div>');
	});

	it('領域の名前をエスケープする', () => {
		const html = renderManual('| a |\n|---|\n| 1 |\n', 'a"b<c&d', 'ja');
		expect(html).toContain('aria-label="a&quot;b&lt;c&amp;d"');
	});

	it('表が無ければ包まない', () => {
		expect(renderManual('# 見出し\n\n本文です。\n', '表', 'ja')).not.toContain('table-scroll');
	});

	it('原典のファイル名で書いたリンクをヘルプのページへ向ける', () => {
		expect(renderManual('[設置する](02-setup.md)', '表', 'ja')).toContain(
			'<a href="/help/setup">設置する</a>'
		);
		expect(renderManual('[戻す](07-maintenance.md#backup)', '表', 'ja')).toContain(
			'href="/help/maintenance#backup"'
		);
	});

	it('ほかのリンクはそのまま残す', () => {
		expect(renderManual('[例](https://example.com/a.md)', '表', 'ja')).toContain(
			'href="https://example.com/a.md"'
		);
	});

	it('画像を本文の言語の画像の API へ向ける', () => {
		const html = renderManual('![画面](images/intro-overview.webp)', '表', 'en');
		expect(html).toContain(
			'<img loading="lazy" srcset="/api/v1/help/images/intro-overview.webp?locale=en 2x" alt="画面">'
		);
	});
});
