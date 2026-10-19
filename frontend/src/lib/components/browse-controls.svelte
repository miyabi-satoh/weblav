<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { ClassValue } from 'svelte/elements';
	import BrowseFilterInput from '$lib/components/browse-filter-input.svelte';
	import BrowseLayoutToggle from '$lib/components/browse-layout-toggle.svelte';

	// 一覧の上の段 (並び順・名前の絞り込み・軸・リストとタイルの切り替え。→ docs/ui.md「UI 全般」)。
	// リストとタイルの切り替えは、いつも1段目の右端に置く。
	// 狭い幅では、並び順と切り替えを1段目に、絞り込みを2段目に幅いっぱいで置く (並び順が無ければ、絞り込みと切り替えで1段)。
	// 広い幅では、並び順・絞り込み・軸を左から並べ、切り替えを右端に置く。

	type Props = {
		/** 並び順の選択。効かない一覧では渡さない。 */
		sort?: Snippet;
		/** 絞り込みの後に並べるもの (アーカイブの軸など)。 */
		children?: Snippet;
		layoutToggle?: boolean;
		class?: ClassValue;
	};

	let { sort, children, layoutToggle = true, class: className }: Props = $props();
</script>

<div class={['flex items-start gap-2.5 max-sm:flex-wrap sm:justify-between sm:gap-3', className]}>
	<!-- 狭い幅では中の物を外の並びに出し (contents)、切り替えを並び順の直後へ回せるようにする。 -->
	<div class="flex flex-wrap gap-2.5 max-sm:contents">
		{#if sort}
			<div class="max-sm:order-first">{@render sort()}</div>
		{/if}
		<BrowseFilterInput class={['w-40', sort ? 'max-sm:basis-full' : 'max-sm:flex-1']} />
		{@render children?.()}
	</div>
	{#if layoutToggle}
		<div class={['ml-auto shrink-0', sort && 'max-sm:order-first']}>
			<BrowseLayoutToggle />
		</div>
	{/if}
</div>
