<script lang="ts">
	import { untrack } from 'svelte';
	import { refreshAll } from '$app/navigation';
	import { WindowVirtualizer } from 'virtua/svelte';
	import { client } from '$lib/api/client';
	import type { components } from '$lib/api/schema';
	import { excludedChunks } from '$lib/archive-list-request';
	import { ArchivePagedList } from '$lib/archive-paged-list.svelte';
	import { archiveRescan } from '$lib/archive-rescan.svelte';
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
	import { adminListBandClass, adminPageClass, pageEmptyTextClass } from '$lib/page-layout';
	import * as m from '$lib/paraglide/messages.js';
	import type { PageProps } from './$types';

	type ExcludedItem = components['schemas']['ExcludedItem'];
	type ExcludedItemsResponse = components['schemas']['ExcludedItemsResponse'];

	// パターンで外れて閲覧画面に出ないファイルを、パスで並べる (→ docs/ui.md「対象外のタブ」)。
	// ファイルは開かない。中身は共有の画面で確かめる。語の取り直しはアイテムのタブと同じ形。
	let { data }: PageProps = $props();

	const listFilter = new ListFilterState();
	provideListFilter(listFilter);
	const errorDialog = new ErrorDialogState();

	/** 一覧に効いている語。打ち止めて 0.3 秒後に送る (→ docs/ui.md「一覧の絞り込み」)。 */
	let appliedName = $derived(data.nameFilter);
	let refetched = $state.raw<{
		data: typeof data;
		name: string;
		view: ExcludedItemsResponse;
	} | null>(null);
	let fromData = $derived(appliedName === data.nameFilter);
	let view = $derived(
		!fromData && refetched?.data === data && refetched.name === appliedName
			? refetched.view
			: data.view
	);
	let nameActive = $derived(appliedName.trim() !== '');

	let list = $derived(new ArchivePagedList<ExcludedItem>(view, excludedChunks(data.contentId)));
	$effect(() => {
		const current = list;
		return () => current.dispose();
	});
	let indexes = $derived(Array.from({ length: list.total }, (_, index) => index));

	async function refetch() {
		const requestedFor = data;
		const name = appliedName;
		const result = await errorDialog.attempt(() =>
			client.GET('/api/v1/contents/{id}/excluded-items', {
				params: { path: { id: data.contentId }, query: { filter: name } }
			})
		);
		const next = result?.data;
		if (!next || requestedFor !== data || name !== appliedName) return;
		refetched = { data, name, view: next };
	}

	$effect(() => {
		const typed = listFilter.value.trim();
		if (typed === appliedName.trim()) return;
		const timer = setTimeout(() => {
			appliedName = typed;
			if (!fromData) void refetch();
		}, 300);
		return () => clearTimeout(timer);
	});

	async function reload() {
		await refreshAll();
		if (!fromData) await refetch();
	}

	$effect(() => {
		if (list.expired) void reload();
	});

	// 再スキャンが終わったら、語で取り直した一覧も取り直す (読み込みの分はレイアウトが取り直している)。
	const rescan = archiveRescan();
	let seenFinished = untrack(() => rescan.finished);
	$effect(() => {
		const finished = rescan.finished;
		if (finished === seenFinished) return;
		seenFinished = finished;
		untrack(() => {
			if (!fromData) void refetch();
		});
	});

	let patternDialogOpen = $state(false);
</script>

<div class={adminPageClass}>
	<div class="flex flex-col">
		<BrowseControls layoutToggle={false} />
		{#if nameActive}
			<div class="mt-2 flex min-h-11 items-center justify-between gap-3 text-sm">
				<p class="text-muted-foreground">
					{m.archive_excluded_count_filtered({
						shown: formatNumber(view.total),
						count: formatNumber(view.totalAll)
					})}
				</p>
				<Button
					type="button"
					variant="outline"
					size="sm"
					onclick={() => (patternDialogOpen = true)}
				>
					{m.pattern_add_button()}
				</Button>
			</div>
		{/if}
		{#if list.stale}
			<ArchiveStaleNotice onreload={() => void reload()} />
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
	filterText={appliedName}
	onsaved={reload}
/>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
