<script lang="ts">
	import { browseLayout } from '$lib/browse-layout.svelte';
	import { browseRowSubtitleClass } from '$lib/list-row';
	import type { RowLocation } from '$lib/row-location';
	import RowLocationButton from '$lib/components/row-location.svelte';
	import SeparatedText from '$lib/components/separated-text.svelte';

	// 行の2段目 (→ $lib/list-row.ts)。頭の印・場所・文字を「·」で区切って並べる。何も無ければ段ごと出さない。
	let {
		prefix,
		location,
		text
	}: {
		/** 頭に置く短い印 (「本人のみ」)。 */
		prefix?: string;
		/** 検索の結果の、どこにあるか。押せるので、行のリンクの上に重ねる。 */
		location?: RowLocation;
		/** 配列なら「·」で区切って並べる。 */
		text?: string | string[] | null;
	} = $props();

	let hasText = $derived(Array.isArray(text) ? text.length > 0 : !!text);
	// タイルは1行に収める。場所は親を残して切るので、段ごと切らずに、中の項目がそれぞれ縮む並びにする。
	let clipParts = $derived(!!location && browseLayout.tile);

	// 区切りの前後の空白は式で出す。ブロックの内側の端に書いた空白は、Svelte が文字の参照でも落とすため。
	const SPACE = ' ';
</script>

{#snippet separator()}{SPACE}<span aria-hidden="true">·</span>{SPACE}{/snippet}

{#if prefix || location || hasText}
	<span class={[browseRowSubtitleClass(), clipParts && 'flex justify-center gap-1']}>
		{#if prefix}
			<span class="shrink-0">{prefix}</span>
			{#if location || hasText}{@render separator()}{/if}
		{/if}
		{#if location}
			<RowLocationButton {location} />
			{#if hasText}{@render separator()}{/if}
		{/if}
		{#if hasText}
			<span class={clipParts ? 'min-w-0 shrink-3 truncate' : undefined}
				><SeparatedText text={text ?? ''} /></span
			>
		{/if}
	</span>
{/if}
