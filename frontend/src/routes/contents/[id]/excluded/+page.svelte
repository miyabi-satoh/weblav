<script lang="ts">
	import { tick } from 'svelte';
	import { WindowVirtualizer, type WindowVirtualizerHandle } from 'virtua/svelte';
	import { client } from '#lib/api/client.ts';
	import type { components } from '#lib/api/schema.js';
	import { excludedChunks } from '#lib/archive-list-request.ts';
	import { ArchivePagedList } from '#lib/archive-paged-list.svelte.ts';
	import ArchiveRowPlaceholder from '#lib/components/archive-row-placeholder.svelte';
	import ArchiveStaleNotice from '#lib/components/archive-stale-notice.svelte';
	import BrowseControls from '#lib/components/browse-controls.svelte';
	import ErrorDialog from '#lib/components/error-dialog.svelte';
	import FilterHighlight from '#lib/components/filter-highlight.svelte';
	import ListFilterEmpty from '#lib/components/list-filter-empty.svelte';
	import PatternAddDialog from '#lib/components/pattern-add-dialog.svelte';
	import { Button } from '#lib/components/ui/button/index.ts';
	import { ErrorDialogState } from '#lib/error-dialog-state.svelte.ts';
	import { formatNumber } from '#lib/format.ts';
	import { ListFilterState, provideListFilter } from '#lib/list-filter.svelte.ts';
	import { NameFilteredView } from '#lib/name-filtered-view.svelte.ts';
	import {
		adminListBandClass,
		adminPageClass,
		listCountRowClass,
		pageEmptyTextClass
	} from '#lib/page-layout.ts';
	import * as m from '#lib/paraglide/messages.js';
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
	// 要素の高さには上限があり (Firefox で約1,790万 px)、40万行ほどで最後まで届かなくなるので、
	// 一覧の表示と同じく10万行ごとにページを分ける (→ docs/archive.md「画面が見える辺りを取りに行く」)。
	const PAGE_ROWS = 100_000;
	let pageCount = $derived(Math.max(1, Math.ceil(list.total / PAGE_ROWS)));
	let page = $state(0);
	// 取り直して今のページが無くなったら、最後のページにする。
	$effect(() => {
		if (page >= pageCount) page = pageCount - 1;
	});
	let indexes = $derived(
		Array.from(
			{ length: Math.max(0, Math.min(PAGE_ROWS, list.total - page * PAGE_ROWS)) },
			(_, i) => page * PAGE_ROWS + i
		)
	);
	let virtualizer = $state<WindowVirtualizerHandle | undefined>();

	/** ページを移り、その先頭から見せる。下のボタンで移ったときに、新しいページの途中から始まらないように。 */
	async function goToPage(next: number) {
		page = next;
		await tick();
		virtualizer?.scrollToIndex(0);
	}

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
			<div class={listCountRowClass}>
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
			{#if pageCount > 1}{@render pageNav()}{/if}
			<div role="list" class="border-t border-divider bg-background">
				<!-- ページが変わると行がすべて変わるので、測った高さを捨てて作り直す。 -->
				{#key page}
					<WindowVirtualizer bind:this={virtualizer} data={indexes} getKey={(index) => index}>
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
				{/key}
			</div>
			{#if pageCount > 1}{@render pageNav()}{/if}
		{/if}
	</div>
</div>

{#snippet pageNav()}
	<div class="flex flex-wrap items-center justify-center gap-3 py-4">
		<Button
			type="button"
			variant="outline"
			disabled={page === 0}
			onclick={() => void goToPage(page - 1)}
		>
			{m.archive_view_previous_page({ count: formatNumber(PAGE_ROWS) })}
		</Button>
		<span class="text-sm text-muted-foreground">
			{m.archive_view_page_position({ page: page + 1, pages: pageCount })}
		</span>
		<Button
			type="button"
			variant="outline"
			disabled={page >= pageCount - 1}
			onclick={() => void goToPage(page + 1)}
		>
			{m.archive_view_next_page({ count: formatNumber(PAGE_ROWS) })}
		</Button>
	</div>
{/snippet}

<PatternAddDialog
	bind:open={patternDialogOpen}
	content={data.content}
	filterText={named.applied}
	initialTarget={data.content.includePatterns ? 'include' : 'exclude'}
	note={m.pattern_add_excluded_note()}
	onsaved={() => named.refresh()}
/>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
