<script lang="ts">
	import { WindowVirtualizer } from 'virtua/svelte';
	import { client } from '$lib/api/client';
	import type { components } from '$lib/api/schema';
	import { excludedChunks } from '$lib/archive-list-request';
	import { ArchivePagedList } from '$lib/archive-paged-list.svelte';
	import ArchiveRowPlaceholder from '$lib/components/archive-row-placeholder.svelte';
	import ArchiveStaleNotice from '$lib/components/archive-stale-notice.svelte';
	import BrowseControls from '$lib/components/browse-controls.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import FilterHighlight from '$lib/components/filter-highlight.svelte';
	import ListFilterEmpty from '$lib/components/list-filter-empty.svelte';
	import PatternAddDialog from '$lib/components/pattern-add-dialog.svelte';
	import { Button } from '$lib/components/ui/button';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { formatNumber } from '$lib/format';
	import { ListFilterState, provideListFilter } from '$lib/list-filter.svelte';
	import { NameFilteredView } from '$lib/name-filtered-view.svelte';
	import { adminListBandClass, adminPageClass, pageEmptyTextClass } from '$lib/page-layout';
	import * as m from '$lib/paraglide/messages.js';
	import type { PageProps } from './$types';

	type ExcludedItem = components['schemas']['ExcludedItem'];
	type ExcludedItemsResponse = components['schemas']['ExcludedItemsResponse'];

	// パターンで外れて閲覧画面に出ないファイルを、パスで並べる (→ docs/ui.md「対象外のタブ」)。
	// ファイルは開かない。中身は共有の画面で確かめる。
	let { data }: PageProps = $props();

	const listFilter = new ListFilterState();
	provideListFilter(listFilter);
	const errorDialog = new ErrorDialogState();

	const named = new NameFilteredView<typeof data, ExcludedItemsResponse>({
		data: () => data,
		viewOf: (current) => current.view,
		typed: () => listFilter.value,
		fetch: async (current, name) => {
			const result = await errorDialog.attempt(() =>
				client.GET('/api/v1/contents/{id}/excluded-items', {
					params: { path: { id: current.contentId }, query: { filter: name } }
				})
			);
			return result?.data;
		}
	});
	let view = $derived(named.view);
	let nameActive = $derived(named.applied.trim() !== '');

	let list = $derived(new ArchivePagedList<ExcludedItem>(view, excludedChunks(data.contentId)));
	$effect(() => {
		const current = list;
		return () => current.dispose();
	});
	let indexes = $derived(Array.from({ length: list.total }, (_, index) => index));

	$effect(() => {
		if (list.expired) void named.refresh();
	});

	let patternDialogOpen = $state(false);
</script>

<div class={adminPageClass}>
	<div class="flex flex-col">
		<BrowseControls layoutToggle={false} />
		{#if view.totalAll > 0 || nameActive}
			<!-- この一覧の数。見出しの「○件のファイル」は見えるものの数なので、別に出す。 -->
			<div class="mt-2 flex min-h-11 items-center justify-between gap-3 text-sm">
				<p class="text-muted-foreground">
					{nameActive
						? m.archive_excluded_count_filtered({
								shown: formatNumber(view.total),
								count: formatNumber(view.totalAll)
							})
						: m.archive_excluded_count({ count: formatNumber(view.totalAll) })}
				</p>
				{#if nameActive}
					<Button
						type="button"
						variant="outline"
						size="sm"
						onclick={() => (patternDialogOpen = true)}
					>
						{m.pattern_add_button()}
					</Button>
				{/if}
			</div>
		{/if}
		{#if list.stale}
			<ArchiveStaleNotice onreload={() => void named.refresh()} />
		{/if}
	</div>

	<div class={[adminListBandClass, 'mt-4']}>
		{#if view.totalAll === 0}
			<p class={pageEmptyTextClass}>{m.archive_excluded_empty()}</p>
		{:else if view.total === 0}
			<ListFilterEmpty />
		{:else}
			<div role="list" class="border-t border-divider bg-background">
				<WindowVirtualizer data={indexes} getKey={(index) => index}>
					{#snippet children(index)}
						{@const item = list.get(index)}
						<div
							role="listitem"
							aria-setsize={list.total}
							aria-posinset={index + 1}
							class="border-b border-divider"
						>
							{#if item}
								<p class="flex min-h-11 items-center px-4 py-2 text-sm wrap-anywhere md:px-6">
									<span><FilterHighlight text={item.relPath} /></span>
								</p>
							{:else}
								<ArchiveRowPlaceholder {list} {index} missing={item === null} />
							{/if}
						</div>
					{/snippet}
				</WindowVirtualizer>
			</div>
		{/if}
	</div>
</div>

<PatternAddDialog
	bind:open={patternDialogOpen}
	content={data.content}
	filterText={named.applied}
	initialTarget={data.content.includePatterns ? 'include' : 'exclude'}
	note={m.pattern_add_excluded_note()}
	onsaved={() => named.refresh()}
/>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
