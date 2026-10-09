<script lang="ts">
	import { highlightSegments } from '$lib/list-filter';
	import { listFilter } from '$lib/list-filter.svelte';

	// 一覧の行のタイトル。ページ内の絞り込みで当たった文字を目立たせる (→ docs/ui.md「一覧の絞り込み」)。
	// 間を空けて当たった行も、なぜ残ったかが読み取れるようにするため。
	let { text }: { text: string } = $props();

	const filter = listFilter();
	let hits = $derived(filter?.active ? (filter.match(text) ?? []) : []);
</script>

{#if hits.length === 0}{text}{:else}{#each highlightSegments(text, hits) as segment, index (index)}{#if segment.hit}<mark
				class="bg-transparent font-bold text-primary">{segment.text}</mark
			>{:else}{segment.text}{/if}{/each}{/if}
