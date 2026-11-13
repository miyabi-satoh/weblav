<script lang="ts">
	import { refreshAll } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { client } from '#lib/api/client.ts';
	import {
		archiveItemManageDownloadHref,
		archiveItemManageThumbnailHref,
		archiveManageFileHref
	} from '#lib/api/urls.ts';
	import type { components } from '#lib/api/schema.js';
	import { NameFilteredView } from '#lib/name-filtered-view.svelte.ts';
	import { Button } from '#lib/components/ui/button/index.ts';
	import PatternAddDialog from '#lib/components/pattern-add-dialog.svelte';
	import { ErrorDialogState } from '#lib/error-dialog-state.svelte.ts';
	import { linksFileHref } from '#lib/links-file.ts';
	import { ArchivePagedList } from '#lib/archive-paged-list.svelte.ts';
	import { archiveListInit, manageChunks, manageShuffle } from '#lib/archive-list-request.ts';
	import ArchiveStaleNotice from '#lib/components/archive-stale-notice.svelte';
	import ListFilterEmpty from '#lib/components/list-filter-empty.svelte';
	import { ListFilterState, provideListFilter } from '#lib/list-filter.svelte.ts';
	import { adminListBandClass, adminPageClass, pageEmptyTextClass } from '#lib/page-layout.ts';
	import * as m from '#lib/paraglide/messages.js';
	import ArchiveViewControls from '#lib/components/archive-view-controls.svelte';
	import ArchiveViewList, {
		type ArchiveItemUrls,
		type ArchiveViewRow
	} from '#lib/components/archive-view-list.svelte';
	import ErrorDialog from '#lib/components/error-dialog.svelte';
	import ListRowSubtitle from '#lib/components/list-row-subtitle.svelte';
	import WarningBand from '#lib/components/warning-band.svelte';
	import type { PageProps } from './$types';

	type ManageItem = components['schemas']['ArchiveViewItem'];
	type ManageArchiveViewResponse = components['schemas']['ManageArchiveViewResponse'];

	// アーカイブのファイルのタブ (→ docs/ui.md「アーカイブのファイルのタブ」)。閲覧の画面と同じ部品で並べ、
	// パスの表示と「パターンに追加...」を重ねる。見出し (タイトル・件数・再スキャン) とタブは [id]/+layout.svelte が出す。
	let { data }: PageProps = $props();

	const listFilter = new ListFilterState();
	provideListFilter(listFilter);

	const errorDialog = new ErrorDialogState();

	let contentId = $derived(data.contentId);

	/** 管理の権限で開く口。 */
	const MANAGE_URLS: ArchiveItemUrls = {
		download: archiveItemManageDownloadHref,
		thumbnail: archiveItemManageThumbnailHref,
		file: archiveManageFileHref
	};

	let listView = $state<ReturnType<typeof ArchiveViewList<ManageItem>> | undefined>();

	const named = new NameFilteredView<typeof data, ManageArchiveViewResponse>({
		data: () => data,
		viewOf: (current) => current.view,
		typed: () => listFilter.value,
		conditions: (current) => JSON.stringify([current.filters, current.sort]),
		position: () => listView?.firstVisibleIndex() ?? 0,
		restore: (position) => void listView?.scrollToIndex(position),
		fetch: async (current, name) => {
			const requestInit = archiveListInit(current.contentId, current.filters, current.sort, name);
			// 取れなかったら理由を出す。黙ると、空の行が出たまま再読み込みのボタンも効かないように見えるため。
			const result = await errorDialog.attempt(() =>
				client.GET('/api/v1/contents/{id}/manage-archive', requestInit)
			);
			return result?.data;
		}
	});
	let view = $derived(named.view);

	let list = $derived(
		new ArchivePagedList<ManageItem>(
			view,
			manageChunks(data.contentId),
			manageShuffle(data.contentId)
		)
	);
	$effect(() => {
		const current = list;
		return () => current.dispose();
	});

	// 結果が捨てられた (しばらく使わなかった) ら、見ていた所から取り直す。
	$effect(() => {
		if (list.expired) {
			if (named.fromData) void refreshAll();
			else void named.refetch(true);
		}
	});

	function linksHref(row: ArchiveViewRow<ManageItem>): string {
		return linksFileHref(contentId, { item: row.item.id }, data.filters);
	}

	let nameActive = $derived(named.applied.trim() !== '');

	let patternDialogOpen = $state(false);
</script>

<!-- 閲覧の画面と同じ軸の値に、パスを足す。 -->
{#snippet itemSubtitle(row: ArchiveViewRow<ManageItem>)}
	<ListRowSubtitle
		text={[row.item.subtitle ?? '', row.item.relPath].filter((part) => part !== '')}
	/>
{/snippet}

<div class={adminPageClass}>
	<div class="flex flex-col gap-4">
		{#if view.folderMissing && view.totalAll > 0}
			<WarningBand class="rounded-md border">
				<p class="text-sm leading-relaxed">{m.error_detail_archive_folder_missing()}</p>
			</WarningBand>
		{/if}

		<div>
			<ArchiveViewControls
				axes={view.axes}
				filters={data.filters}
				sort={data.sort}
				path={resolve('/contents/[id]/items', { id: String(contentId) })}
				total={nameActive ? view.totalAll : view.total}
				shown={nameActive ? view.total : undefined}
				countOnlyWhenFiltered
				hasItems={view.totalAll > 0}
				folderMissing={view.folderMissing}
			>
				{#snippet countAction()}
					{#if nameActive}
						<Button
							type="button"
							variant="outline"
							size="sm"
							class="ml-auto"
							onclick={() => (patternDialogOpen = true)}
						>
							{m.pattern_add_button()}
						</Button>
					{/if}
				{/snippet}
			</ArchiveViewControls>
			{#if list.stale}
				<ArchiveStaleNotice onreload={() => void named.refresh()} />
			{/if}
		</div>
	</div>

	<div class={adminListBandClass}>
		{#if view.totalAll === 0}
			<p class={pageEmptyTextClass}>
				{view.folderMissing ? m.error_detail_archive_folder_missing() : m.archive_items_empty()}
			</p>
		{:else if view.total === 0}
			{#if nameActive}
				<ListFilterEmpty />
			{:else}
				<p class={pageEmptyTextClass}>{m.archive_view_empty()}</p>
			{/if}
		{:else}
			<ArchiveViewList
				bind:this={listView}
				{list}
				archiveId={contentId}
				{linksHref}
				urls={MANAGE_URLS}
				subtitle={itemSubtitle}
			/>
		{/if}
	</div>
</div>

<PatternAddDialog
	bind:open={patternDialogOpen}
	content={data.content}
	filterText={named.applied}
	onsaved={() => named.refresh()}
/>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
