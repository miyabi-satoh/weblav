<script lang="ts">
	import BrowseControls from '$lib/components/browse-controls.svelte';
	import BrowseSortSelect from '$lib/components/browse-sort.svelte';
	import ContentList from '$lib/components/content-list.svelte';
	import { ListFilterState, provideListFilter } from '$lib/list-filter.svelte';
	import { resolve } from '$app/paths';
	import { SORT_OPTIONS, browseSortHref, type BrowseSort } from '$lib/browse-sort';
	import { browseGutterClass } from '$lib/list-row';
	import { pageEmptyTextClass, pageHeadingClass } from '$lib/page-layout';
	import { pageTitle } from '$lib/page-title';
	import * as m from '$lib/paraglide/messages.js';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	// 閲覧の一覧と同じ部品で並べ、並び順・絞り込み・タイルとリストの切り替えも同じにする (→ docs/ui.md「UI 全般」)。
	const listFilter = new ListFilterState();
	provideListFilter(listFilter);

	function sortHref(sort: BrowseSort): string {
		// 並び順を変えても、打った絞り込みは残す。
		return browseSortHref(resolve('/hidden'), sort, listFilter.query);
	}
</script>

<svelte:head><title>{pageTitle(m.hidden_title())}</title></svelte:head>

{#snippet sortSelect()}
	<BrowseSortSelect value={data.sort} options={SORT_OPTIONS} href={sortHref} />
{/snippet}

<div class="py-6">
	<div class={browseGutterClass}>
		<h1 class={pageHeadingClass}>{m.hidden_title()}</h1>
		{#if data.hidden.length > 0}
			<BrowseControls class="mb-4" sort={sortSelect} />
		{/if}
	</div>

	{#if data.hidden.length > 0}
		<ContentList entries={data.hidden} actions />
	{:else}
		<p class={pageEmptyTextClass}>{m.hidden_empty()}</p>
	{/if}
</div>
