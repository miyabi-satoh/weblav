<script lang="ts">
	import * as m from '$lib/paraglide/messages.js';
	import { listFilter } from '$lib/list-filter.svelte';
	import { pageEmptyTextClass } from '$lib/page-layout';

	// 一覧のページ内の絞り込みで、当たる行が無いときの表示 (→ docs/ui.md「一覧の絞り込み」)。
	const filter = listFilter();
</script>

{#if filter}
	<p class={pageEmptyTextClass}>{m.browse_filter_empty({ query: filter.value.trim() })}</p>
{/if}
