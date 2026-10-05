import { describe, expect, it } from 'vitest';
import { renderMarkdownPreview } from './markdown-preview';

// 無害化は DOM が要るので、ブラウザで走らせる (ファイル名の `.svelte.spec` → vite.config.ts)。

/** 組んだ HTML を要素にして返す。 */
async function render(markdown: string): Promise<HTMLElement> {
	const container = document.createElement('div');
	container.innerHTML = (await renderMarkdownPreview(markdown, '表')) ?? '';
	return container;
}

describe('renderMarkdownPreview', () => {
	it('中の HTML のスクリプトとイベントの属性を外す', async () => {
		const html = await render(
			'<script>alert(1)</script>\n\n<img src="https://example.com/a.png" onerror="alert(2)">'
		);
		expect(html.querySelector('script')).toBeNull();
		expect(html.querySelector('[onerror]')).toBeNull();
		expect(html.querySelector('img')?.getAttribute('src')).toBe('https://example.com/a.png');
	});

	it('Markdown が出さない要素・属性と、アプリの class を外す', async () => {
		const html = await render(
			[
				'<style>body { display: none }</style>',
				'<form action="https://example.com"><input type="password"></form>',
				'<div class="fixed inset-0" style="z-index: 99">覆い</div>',
				'',
				'- [x] 済んだこと',
				'',
				'```ts',
				'const a = 1;',
				'```'
			].join('\n')
		);
		expect(html.querySelector('style, form, [style]')).toBeNull();
		expect(html.querySelector('input[type="password"]')).toBeNull();
		expect(html.querySelector('.fixed')).toBeNull();
		expect(html.textContent).toContain('覆い');
		expect(html.querySelector('input[type="checkbox"]')?.hasAttribute('disabled')).toBe(true);
		expect(html.querySelector('code')?.className).toBe('language-ts');
	});

	it('画像の srcset のような、ほかの URL の属性も残さない', async () => {
		const html = await render('<img src="https://example.com/a.png" srcset="/api/v1/x 2x">');
		expect(html.querySelector('img')?.hasAttribute('srcset')).toBe(false);
	});

	it('長すぎるテキストは組まない', async () => {
		expect(await renderMarkdownPreview('a'.repeat(256 * 1024 + 1), '表')).toBeUndefined();
	});

	it('http・https・mailto のリンクだけを残し、別のタブで開く', async () => {
		const html = await render(
			'[外](https://example.com) [メール](mailto:a@example.com) [危ない](javascript:alert(1)) [相対](other.md)'
		);
		const links = [...html.querySelectorAll('a')];
		expect(links.map((link) => link.getAttribute('href'))).toEqual([
			'https://example.com',
			'mailto:a@example.com',
			null,
			null
		]);
		expect(links[0].getAttribute('target')).toBe('_blank');
		expect(links[0].getAttribute('rel')).toBe('noopener noreferrer');
		expect(links[3].textContent).toBe('相対');
	});

	it('相対パスの画像は読み込まず、代わりの文字にする', async () => {
		const html = await render('![図1](images/a.png)');
		expect(html.querySelector('img')).toBeNull();
		expect(html.textContent).toContain('図1');
	});

	it('表を、キーボードで送れる領域で包む', async () => {
		const html = await render('| a | b |\n| - | - |\n| 1 | 2 |');
		const region = html.querySelector('.table-scroll');
		expect(region?.getAttribute('tabindex')).toBe('0');
		expect(region?.getAttribute('aria-label')).toBe('表');
	});
});
