<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { ClassValue } from 'svelte/elements';
	import BrowseFilterInput from '$lib/components/browse-filter-input.svelte';
	import BrowseLayoutToggle from '$lib/components/browse-layout-toggle.svelte';

	// 一覧の上の段 (→ docs/ui.md「UI 全般」)。
	// 1段目は、どの一覧でも並び順・名前の絞り込み (余りまで伸ばす)・リストとタイルの切り替え。
	// アーカイブの軸は2段目に並べる。1段目と混ぜると、軸の数と幅しだいで絞り込みの大きさと段の切れ目が変わるため。
	// 狭い幅では、並び順と切り替えを1段目に、絞り込みを次に幅いっぱいで置く (並び順が無ければ、絞り込みと切り替えで1段)。

	type Props = {
		/** 並び順の選択。効かない一覧では渡さない。 */
		sort?: Snippet;
		/** 2段目に並べる絞り込み (アーカイブの軸など)。無ければ渡さない。 */
		filters?: Snippet;
		layoutToggle?: boolean;
		class?: ClassValue;
	};

	let { sort, filters, layoutToggle = true, class: className }: Props = $props();
</script>

<div class={['flex flex-col gap-2.5', className]}>
	<div class="flex items-start gap-2.5 max-sm:flex-wrap">
		{#if sort}
			<div class="shrink-0">{@render sort()}</div>
		{/if}
		<!-- 狭い幅で並び順があるときは、切り替えを並び順の隣へ回し、絞り込みを次の段に幅いっぱいで置く。 -->
		<BrowseFilterInput class={['min-w-40 flex-1', sort && 'max-sm:order-last max-sm:basis-full']} />
		{#if layoutToggle}
			<div class="ml-auto shrink-0"><BrowseLayoutToggle /></div>
		{/if}
	</div>
	{#if filters}
		<div class="flex flex-wrap gap-2.5">{@render filters()}</div>
	{/if}
</div>
