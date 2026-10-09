/**
 * テキストのビューアーで、Markdown のファイルを組んで見せる (→ docs/ui.md「PDF・動画・テキストのビューアー」)。
 * marked と DOMPurify は、プレビューを開いたときに読み込む。
 */

import { MARKDOWN_EXTENSIONS, MAX_FORMATTED_LENGTH } from '$lib/code-highlight';
import { fileExtension } from '$lib/file-kind';

/** Markdown として組んで見せるファイルか。 */
export function isMarkdownFileName(fileName: string): boolean {
	const extension = fileExtension(fileName);
	return extension !== undefined && MARKDOWN_EXTENSIONS.includes(extension);
}

/**
 * 残す要素と属性。Markdown が出す形だけを許し、ほかは中身の文字だけを残す。
 * DOMPurify の既定は `<style>`・`<form>`・`style` 属性なども通し、アプリの画面を覆ったり書き換えたりできるため。
 * `input` は、marked がタスクリスト (`- [ ]`) に出すチェックボックス。
 */
const ALLOWED_TAGS = [
	'a',
	'blockquote',
	'br',
	'code',
	'del',
	'details',
	'div',
	'em',
	'h1',
	'h2',
	'h3',
	'h4',
	'h5',
	'h6',
	'hr',
	'img',
	'input',
	'kbd',
	'li',
	'ol',
	'p',
	'pre',
	's',
	'span',
	'strong',
	'sub',
	'summary',
	'sup',
	'table',
	'tbody',
	'td',
	'th',
	'thead',
	'tr',
	'ul'
];
const ALLOWED_ATTR = [
	'align',
	'alt',
	'aria-label',
	'checked',
	'class',
	'colspan',
	'disabled',
	'href',
	'open',
	'role',
	'rowspan',
	'src',
	'start',
	'tabindex',
	'title',
	'type'
];

/** リンク先として残す URL。ほかの形 (相対パス・`javascript:` など) はリンクを外して文字だけ残す。 */
const LINK_URL = /^(https?:|mailto:)/i;
/** 画像として読み込む URL。 */
const IMAGE_URL = /^https?:/i;
/** 残す `class`。アプリの画面の class (Tailwind) を書かれると、画面を覆う形にもできるため絞る。 */
const ALLOWED_CLASS = /^(language-\S+|table-scroll)$/;

/**
 * `text` を、無害化した HTML にする。組まないもの (長すぎる) は `undefined`。
 *
 * @param tableLabel 表を包む領域の名前 (→ $lib/manual-markdown.ts の `ScrollTableRenderer`)。
 */
export async function renderMarkdownPreview(
	text: string,
	tableLabel: string
): Promise<string | undefined> {
	if (text.length > MAX_FORMATTED_LENGTH) return undefined;
	const [{ marked }, { ScrollTableRenderer, collapseCjkLineBreaks }, { default: DOMPurify }] =
		await Promise.all([import('marked'), import('$lib/manual-markdown'), import('dompurify')]);
	const html = collapseCjkLineBreaks(
		marked.parse(text, { async: false, renderer: new ScrollTableRenderer(tableLabel) })
	);
	const fragment = DOMPurify.sanitize(html, {
		ALLOWED_TAGS,
		ALLOWED_ATTR,
		RETURN_DOM_FRAGMENT: true
	});
	for (const element of fragment.querySelectorAll('[class]')) {
		if (!ALLOWED_CLASS.test(element.getAttribute('class') ?? '')) element.removeAttribute('class');
	}
	for (const input of fragment.querySelectorAll('input')) {
		if (input.getAttribute('type') === 'checkbox') input.setAttribute('disabled', '');
		else input.remove();
	}
	for (const link of fragment.querySelectorAll('a')) {
		const href = link.getAttribute('href');
		if (href === null || !LINK_URL.test(href)) {
			link.removeAttribute('href');
		} else {
			// ビューアーの下の一覧を残したまま、別のタブで開く。
			link.setAttribute('target', '_blank');
			link.setAttribute('rel', 'noopener noreferrer');
		}
	}
	for (const image of fragment.querySelectorAll('img')) {
		const src = image.getAttribute('src');
		if (src === null || !IMAGE_URL.test(src)) {
			image.replaceWith(document.createTextNode(image.getAttribute('alt') ?? ''));
		}
	}
	const container = document.createElement('div');
	container.append(fragment);
	return container.innerHTML;
}
