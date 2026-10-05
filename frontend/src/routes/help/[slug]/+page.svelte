<script lang="ts">
	import type { PageProps } from './$types';
	import { pageTitle } from '$lib/page-title';
	import { renderManual } from '$lib/manual-markdown';
	import * as m from '$lib/paraglide/messages.js';

	let { data }: PageProps = $props();

	// 表を包む領域の名前は画面の造りの一部なので、本文ではなく画面の表示言語で出す。
	let html = $derived(
		renderManual(data.helpPage.body, m.common_scroll_table_label(), data.helpPage.locale)
	);
</script>

<svelte:head><title>{pageTitle(data.helpPage.title)}</title></svelte:head>

<!-- 本文の言語はアプリの表示言語と違うことがあるため、読み上げ・自動翻訳の判断が
     ページ全体の `<html lang>` に引きずられないよう、ここで明示する。
     要求した言語ではなくサーバーが実際に返した言語を使う (訳が無いページは原典に
     落ちるので、要求した方を書くと嘘になる → docs/help.md)。 -->
<article lang={data.helpPage.locale} class="markdown-body help-body text-base leading-relaxed">
	<!-- eslint-disable-next-line svelte/no-at-html-tags -- ソースはdocs/manual/*.mdのみで外部入力を含まない(上のコメント参照) -->
	{@html html}
</article>

<style>
	/* Markdown のプレビューと共通の組み方 (layout.css の markdown-body) との違いだけを書く。 */

	/* 先頭の h1 はページの題。スマートフォン幅では目次の下に本文が続くので、どのページかを
	   ここで示す。 */
	.help-body :global(h1) {
		margin: 0 0 1rem;
	}
	/* 画面の画像。実物の大きさで出し (→ manual-markdown.ts)、本文より広いものだけ縮める。
	   背景と見分けられるよう枠を付ける。 */
	.help-body :global(img) {
		display: block;
		max-width: 100%;
		height: auto;
		border: 1px solid var(--border);
		border-radius: 0.5rem;
	}
	.help-body :global(li img) {
		margin: 0.5rem 0;
	}
	/* 引用 (`>`) は、読み落とすと困る注意の囲みとして使う。
	   差し色ではなく警告色で塗る (→ docs/ui.md「UI 全般」、warning-band.svelte と同じ色)。 */
	.help-body :global(blockquote) {
		margin: 1rem 0;
		padding: 0.75rem 1rem;
		border: 1px solid var(--warning-border);
		border-radius: 0.5rem;
		background: var(--warning);
		color: var(--warning-foreground);
	}
	.help-body :global(blockquote > :first-child) {
		margin-top: 0;
	}
	.help-body :global(blockquote > :last-child) {
		margin-bottom: 0;
	}
	.help-body :global(th) {
		white-space: nowrap;
	}
	/* コードの区画は、ページの地より一段明るいカードの地に枠を付ける。反転した濃い地は
	   本文の中でいちばん強く目立ち、読み手 (開発者に限らない) に身構えさせるため、
	   Microsoft Learn・Apple のサポートと同じ明るい地にする。 */
	.help-body :global(pre) {
		background: var(--card);
	}
</style>
