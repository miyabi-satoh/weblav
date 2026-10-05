<script lang="ts">
	import { Spinner } from '$lib/components/ui/spinner';
	import { highlightCode, highlightCodeBlock, isCodeFileName } from '$lib/code-highlight';
	import { isMarkdownFileName, renderMarkdownPreview } from '$lib/markdown-preview';
	import * as m from '$lib/paraglide/messages.js';

	// ビューアのテキストの本文 (→ docs/ui.md「PDF・動画・テキストのビューア」)。
	let {
		src,
		fileName,
		source = false,
		onerror
	}: {
		src: string;
		fileName: string;
		/** Markdown のファイルを、組まずにソースのまま見せる。ほかのファイルには効かない。 */
		source?: boolean;
		onerror: () => void;
	} = $props();

	/** これより大きいファイルはページに読み込まない。ログのような巨大なファイルで画面を固めないため。 */
	const MAX_BYTES = 2 * 1024 * 1024;

	let text = $state<string>();
	/** 色を付けた HTML。付けないファイルと、付け終わるまでは `undefined`。 */
	let highlighted = $state<string>();
	/** Markdown を組んで無害化した HTML。組み終わるまでは `undefined`、組まない・組めないときは `null`。 */
	let preview = $state<string | null>();

	let isCode = $derived(isCodeFileName(fileName));
	let isMarkdown = $derived(isMarkdownFileName(fileName));
	// 組めないときは、ソースのまま見せる。
	let showPreview = $derived(isMarkdown && !source && preview !== null);

	/**
	 * UTF-8 として読めなければ Shift_JIS で読む。Windows のメモ帳が古くから保存してきた日本語の
	 * テキストは Shift_JIS が多く、サーバーは文字コードを見ずに返すため。
	 */
	function decode(bytes: ArrayBuffer): string {
		try {
			return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
		} catch {
			return new TextDecoder('shift_jis').decode(bytes);
		}
	}

	$effect(() => {
		const url = src;
		const controller = new AbortController();
		text = undefined;
		highlighted = undefined;
		preview = undefined;
		void (async () => {
			try {
				const response = await fetch(url, { signal: controller.signal });
				const length = Number(response.headers.get('content-length'));
				if (!response.ok || length > MAX_BYTES) throw new Error();
				const bytes = await response.arrayBuffer();
				if (bytes.byteLength > MAX_BYTES) throw new Error();
				text = decode(bytes);
			} catch {
				if (!controller.signal.aborted) onerror();
			}
		})();
		return () => controller.abort();
	});

	// 組めなくても色を付けられなくても、素のテキストは読めているので案内は出さない。
	$effect(() => {
		if (text === undefined || !isMarkdown) return;
		let cancelled = false;
		renderMarkdownPreview(text, m.common_scroll_table_label())
			.catch(() => undefined)
			.then((html) => {
				if (!cancelled) preview = html ?? null;
			});
		return () => (cancelled = true);
	});

	// Markdown のソースは、ソースを見せるときに初めて色を付ける。プレビューを待たせないため。
	$effect(() => {
		if (text === undefined || showPreview || highlighted !== undefined) return;
		let cancelled = false;
		highlightCode(fileName, text)
			.catch(() => undefined)
			.then((html) => {
				if (!cancelled) highlighted = html;
			});
		return () => (cancelled = true);
	});

	/** プレビューの中のコードの塊に、言語の指定 (```` ```yaml ````) に合わせて色を付ける。 */
	function highlightCodeBlocks(article: HTMLElement) {
		// 無害化で、コードの塊の class は `language-<言語>` の1つだけになっている (→ $lib/markdown-preview.ts)。
		for (const code of article.querySelectorAll<HTMLElement>('pre > code[class^="language-"]')) {
			const language = code.className.slice('language-'.length);
			void highlightCodeBlock(language, code.textContent)
				.catch(() => undefined)
				.then((html) => {
					// highlight.js が中身をエスケープして返す (→ $lib/code-highlight.ts)。
					if (html !== undefined && code.isConnected) code.innerHTML = html;
				});
		}
	}
</script>

<div class="size-full overflow-auto p-4">
	{#if text === undefined || (showPreview && preview === undefined)}
		<div class="flex size-full items-center justify-center">
			<Spinner class="size-8 text-white" aria-label={m.common_loading()} />
		</div>
	{:else if showPreview}
		<!-- 無害化した HTML (→ $lib/markdown-preview.ts)。 -->
		<!-- eslint-disable svelte/no-at-html-tags -->
		{#key preview}
			<article
				class="markdown-body markdown-preview code-view mx-auto max-w-4xl rounded-md bg-background p-4 text-base leading-relaxed break-words text-foreground sm:p-6"
				{@attach highlightCodeBlocks}
			>
				{@html preview}
			</article>
		{/key}
		<!-- eslint-enable svelte/no-at-html-tags -->
	{:else}
		<!-- highlight.js が中身をエスケープして返すので、{@html} に渡してよい (→ $lib/code-highlight.ts)。 -->
		<!-- eslint-disable svelte/no-at-html-tags -->
		<pre
			class={[
				'mx-auto max-w-4xl rounded-md bg-background p-4 break-words whitespace-pre-wrap text-foreground sm:p-6',
				isCode
					? 'code-view font-mono text-sm leading-6'
					: 'font-sans text-sm leading-6 sm:text-base'
			]}>{#if highlighted !== undefined}{@html highlighted}{:else}{text}{/if}</pre>
		<!-- eslint-enable svelte/no-at-html-tags -->
	{/if}
</div>

<style>
	/* ソースの色 (→ docs/ui.md「PDF・動画・テキストのビューア」)。 */
	.code-view {
		--code-keyword: #cf222e;
		--code-title: #8250df;
		--code-constant: #0550ae;
		--code-string: #0a3069;
		--code-built-in: #953800;
		--code-comment: #6e7781;
		--code-tag: #116329;
		--code-addition-bg: #dafbe1;
		--code-deletion: #82071e;
		--code-deletion-bg: #ffebe9;
	}
	:global(.dark) .code-view {
		--code-keyword: #ff7b72;
		--code-title: #d2a8ff;
		--code-constant: #79c0ff;
		--code-string: #a5d6ff;
		--code-built-in: #ffa657;
		--code-comment: #8b949e;
		--code-tag: #7ee787;
		--code-addition-bg: #033a16;
		--code-deletion: #ffdcd7;
		--code-deletion-bg: #67060c;
	}
	.code-view
		:global(
			:is(
				.hljs-doctag,
				.hljs-keyword,
				.hljs-template-tag,
				.hljs-template-variable,
				.hljs-type,
				.hljs-variable.language_
			)
		) {
		color: var(--code-keyword);
	}
	.code-view :global(:is(.hljs-title, .hljs-title.class_, .hljs-title.function_)) {
		color: var(--code-title);
	}
	.code-view
		:global(
			:is(
				.hljs-attr,
				.hljs-attribute,
				.hljs-literal,
				.hljs-meta,
				.hljs-number,
				.hljs-operator,
				.hljs-variable,
				.hljs-selector-attr,
				.hljs-selector-class,
				.hljs-selector-id,
				.hljs-section
			)
		) {
		color: var(--code-constant);
	}
	.code-view :global(:is(.hljs-regexp, .hljs-string, .hljs-meta .hljs-string)) {
		color: var(--code-string);
	}
	.code-view :global(:is(.hljs-built_in, .hljs-symbol, .hljs-bullet)) {
		color: var(--code-built-in);
	}
	.code-view :global(:is(.hljs-comment, .hljs-code, .hljs-formula)) {
		color: var(--code-comment);
	}
	.code-view :global(:is(.hljs-name, .hljs-quote, .hljs-selector-tag, .hljs-selector-pseudo)) {
		color: var(--code-tag);
	}
	.code-view :global(:is(.hljs-section, .hljs-strong)) {
		font-weight: bold;
	}
	.code-view :global(.hljs-emphasis) {
		font-style: italic;
	}
	.code-view :global(.hljs-addition) {
		color: var(--code-tag);
		background-color: var(--code-addition-bg);
	}
	.code-view :global(.hljs-deletion) {
		color: var(--code-deletion);
		background-color: var(--code-deletion-bg);
	}

	/* Markdown のプレビュー。マニュアルと共通の組み方 (layout.css の markdown-body) との違いだけを書く。
	   引用は、マニュアルのような注意の囲みではなく、ふつうの引用として左に線を引く。 */
	.markdown-preview :global(> :first-child) {
		margin-top: 0;
	}
	.markdown-preview :global(> :last-child) {
		margin-bottom: 0;
	}
	.markdown-preview :global(:is(h4, h5, h6)) {
		margin: 1.25rem 0 0.5rem;
		font-weight: 700;
	}
	.markdown-preview :global(blockquote) {
		margin: 0.75rem 0;
		padding-left: 1rem;
		border-left: 4px solid var(--border);
		color: var(--muted-foreground);
	}
	.markdown-preview :global(img) {
		max-width: 100%;
		height: auto;
	}
	.markdown-preview :global(hr) {
		margin: 1.5rem 0;
		border-top: 1px solid var(--border);
	}
	.markdown-preview :global(code) {
		font-size: 0.875em;
	}
	/* コードの塊の地は本文と同じにし、枠で囲む。地の色を変えると、ソースの色のコントラストが下がるため。 */
	.markdown-preview :global(pre) {
		line-height: 1.5rem;
	}
</style>
