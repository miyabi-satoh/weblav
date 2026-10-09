<script lang="ts">
	import BrowseFilterInput from '$lib/components/browse-filter-input.svelte';
	import { ListFilterState, provideListFilter } from '$lib/list-filter.svelte';
	import { resolve } from '$app/paths';
	import * as m from '$lib/paraglide/messages.js';
	import { SORT_OPTIONS, browseSortHref, type BrowseSort } from '$lib/browse-sort';
	import BrowseBreadcrumb from '$lib/components/browse-breadcrumb.svelte';
	import BrowseLayoutToggle from '$lib/components/browse-layout-toggle.svelte';
	import BrowseSortSelect from '$lib/components/browse-sort.svelte';
	import ContentList from '$lib/components/content-list.svelte';
	import type { PageProps } from './$types';
	import { pageTitle } from '$lib/page-title';
	import { browseControlsClass, browseGutterClass } from '$lib/list-row';
	import { pageHeadingClass, pageEmptyTextClass } from '$lib/page-layout';

	let { data }: PageProps = $props();

	// ページ内の絞り込み (→ docs/ui.md「一覧の絞り込み」)。一覧の部品が context から読む。
	const listFilter = new ListFilterState();
	provideListFilter(listFilter);

	let browse = $derived(data.browse);

	function sortHref(sort: BrowseSort): string {
		// 並び順を変えても、打った絞り込みは残す。
		return browseSortHref(
			resolve('/groups/[id]', { id: String(data.contentId) }),
			sort,
			listFilter.query
		);
	}
</script>

<svelte:head><title>{pageTitle(browse.groupTitle)}</title></svelte:head>

<div class="py-6">
	<div class={browseGutterClass}>
		<!-- パンくずは常にURLナビゲーションで遷移する(folders/[id]/+page.svelteと同じ方針)。
		     ホーム -> 祖先group(ルートに近い順) -> 現在地(リンクなし)。 -->
		<BrowseBreadcrumb ancestors={browse.ancestors}>
			<span aria-hidden="true">/</span>
			<span>{browse.groupTitle}</span>
		</BrowseBreadcrumb>

		<h1 class={pageHeadingClass}>{browse.groupTitle}</h1>

		{#if browse.entries.length > 0}
			<div class={['mb-4', browseControlsClass]}>
				<div class="flex flex-wrap gap-2.5">
					<BrowseSortSelect value={data.sort} options={SORT_OPTIONS} href={sortHref} />
					<BrowseFilterInput />
				</div>
				<BrowseLayoutToggle />
			</div>
		{/if}
	</div>

	{#if browse.entries.length === 0}
		<p class={pageEmptyTextClass}>{m.group_browse_empty()}</p>
	{:else}
		<ContentList entries={browse.entries} />
	{/if}
</div>
