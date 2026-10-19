<script lang="ts">
	import { ListFilterState, provideListFilter } from '$lib/list-filter.svelte';
	import { resolve } from '$app/paths';
	import * as m from '$lib/paraglide/messages.js';
	import { linksFileHref } from '$lib/links-file';
	import { browseGutterClass } from '$lib/list-row';
	import BrowseBreadcrumb from '$lib/components/browse-breadcrumb.svelte';
	import ArchiveViewControls from '$lib/components/archive-view-controls.svelte';
	import ArchiveViewList, { type ArchiveViewRow } from '$lib/components/archive-view-list.svelte';
	import type { PageProps } from './$types';
	import { pageTitle } from '$lib/page-title';
	import { pageHeadingClass, pageEmptyTextClass } from '$lib/page-layout';

	let { data }: PageProps = $props();

	// ページ内の絞り込み (→ docs/ui.md「一覧の絞り込み」)。一覧の部品が context から読む。
	const listFilter = new ListFilterState();
	provideListFilter(listFilter);

	let contentId = $derived(data.contentId);
	let view = $derived(data.view);

	function linksHref(row: ArchiveViewRow): string {
		return linksFileHref(contentId, { item: row.item.id }, data.filters);
	}

	let rows = $derived(view.items.map((item) => ({ archiveId: contentId, item })));

	/** 名前の絞り込みで残る行の数。一覧の部品と同じくタイトルで照らす。 */
	let shownCount = $derived(listFilter.apply(view.items, (item) => item.title).length);
</script>

<svelte:head><title>{pageTitle(view.archiveTitle)}</title></svelte:head>

<div class="py-6">
	<div class={browseGutterClass}>
		<BrowseBreadcrumb ancestors={view.ancestors}>
			<span aria-hidden="true">/</span>
			<span>{view.archiveTitle}</span>
		</BrowseBreadcrumb>

		<h1 class={pageHeadingClass}>{view.archiveTitle}</h1>

		<ArchiveViewControls
			axes={view.axes}
			filters={data.filters}
			sort={data.sort}
			path={resolve('/archives/[id]', { id: String(contentId) })}
			total={view.items.length}
			shown={listFilter.active ? shownCount : undefined}
			hasItems={view.items.length > 0}
			folderMissing={view.folderMissing}
		/>
	</div>

	{#if view.items.length === 0}
		<p class={pageEmptyTextClass}>
			{view.folderMissing ? m.error_detail_archive_folder_missing() : m.archive_view_empty()}
		</p>
	{:else}
		<ArchiveViewList {rows} {linksHref} />
	{/if}
</div>
