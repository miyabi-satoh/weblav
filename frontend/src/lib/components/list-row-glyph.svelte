<script lang="ts">
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import { browseLayout } from '$lib/browse-layout.svelte';
	import { tileCornerClass } from '$lib/list-row';

	// 1列リストの行の末尾に置く、遷移の種類を示すグリフ (→ $lib/list-row.ts)。
	// タイルでは「›」を省き、新規タブの印だけを右上の隅に置く (→ docs/ui.md「UI 全般」)。
	// 印は装飾扱い (aria-hidden) で、タイトルのリンクの外にある。新規タブで開くことは、リンクの中の文字で伝える。
	let { newTab = false }: { newTab?: boolean } = $props();
</script>

{#if newTab}
	<ExternalLinkIcon
		class={[
			'shrink-0 text-muted-foreground',
			browseLayout.tile ? [tileCornerClass, 'size-4'] : 'size-5'
		]}
		strokeWidth={1.8}
	/>
{:else if !browseLayout.tile}
	<ChevronRightIcon class="size-5 shrink-0 text-muted-foreground" strokeWidth={1.8} />
{/if}
