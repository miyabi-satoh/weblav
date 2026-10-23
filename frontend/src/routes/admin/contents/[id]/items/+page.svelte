<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { resolve } from '$app/paths';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import { client } from '$lib/api/client';
	import {
		archiveItemManageDownloadHref,
		archiveItemManageThumbnailHref,
		archiveManageFileHref
	} from '$lib/api/urls';
	import type { components } from '$lib/api/schema';
	import { rescanArchive, waitForRunningScan } from '$lib/archive-scan';
	import { browseLayout } from '$lib/browse-layout.svelte';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { formatNumber } from '$lib/format';
	import { linksFileHref } from '$lib/links-file';
	import { containsTerms } from '$lib/list-filter';
	import { ListFilterState, provideListFilter } from '$lib/list-filter.svelte';
	import { browseRowAboveClass } from '$lib/list-row';
	import { adminPageClass, pageEmptyTextClass } from '$lib/page-layout';
	import * as m from '$lib/paraglide/messages.js';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import * as Select from '$lib/components/ui/select';
	import { Switch } from '$lib/components/ui/switch';
	import ArchiveViewControls from '$lib/components/archive-view-controls.svelte';
	import ArchiveViewList, {
		type ArchiveItemUrls,
		type ArchiveViewRow
	} from '$lib/components/archive-view-list.svelte';
	import BrowseSelectTrigger from '$lib/components/browse-select-trigger.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import ListRowSubtitle from '$lib/components/list-row-subtitle.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import WarningBand from '$lib/components/warning-band.svelte';
	import type { PageProps } from './$types';

	type ManageItem = components['schemas']['ManageArchiveViewItem'];

	// アーカイブのアイテムのタブ (→ docs/ui.md「アーカイブのアイテムのタブ」)。閲覧の画面と同じ部品で並べ、
	// 選択と公開の切り替えを重ねる。見出し (タイトル・件数) とタブは [id]/+layout.svelte が出す。
	let { data }: PageProps = $props();

	const listFilter = new ListFilterState();
	provideListFilter(listFilter);

	const errorDialog = new ErrorDialogState();

	let contentId = $derived(data.contentId);
	let editable = $derived(data.editable);
	let view = $derived(data.view);

	/** 未公開も開ける管理用の口。 */
	const MANAGE_URLS: ArchiveItemUrls = {
		download: archiveItemManageDownloadHref,
		thumbnail: archiveItemManageThumbnailHref,
		file: archiveManageFileHref
	};

	type Status = 'all' | 'published' | 'unpublished';
	/** 公開の状態での絞り込み。軸と違い画面の中だけで絞る (サーバーの件数は公開の状態によらない)。 */
	let status = $state<Status>('all');
	const STATUS_LABELS: Record<Status, () => string> = {
		all: m.archive_view_filter_all,
		published: m.archive_items_status_published,
		unpublished: m.archive_items_status_unpublished
	};

	function matchesStatus(item: ManageItem, value: Status): boolean {
		return value === 'all' || item.published === (value === 'published');
	}

	function statusCount(value: Status): number {
		return view.items.filter((item) => matchesStatus(item, value)).length;
	}

	let items = $derived(view.items.filter((item) => matchesStatus(item, status)));
	let rows = $derived(items.map((item) => ({ archiveId: contentId, item })));
	let byId = $derived(new Map(view.items.map((item) => [item.id, item])));

	/**
	 * 名前の絞り込みは、表示タイトルに加えてパスでも照らす (→ docs/archive.md「アイテムの公開」)。
	 * パスは続いた文字列だけで照らす (→ $lib/list-filter.ts の `containsTerms`)。
	 */
	function matches(row: ArchiveViewRow): boolean {
		return (
			listFilter.match(row.item.title) !== null || containsTerms(row.item.relPath, listFilter.terms)
		);
	}

	let visibleRows = $derived(listFilter.active ? rows.filter(matches) : rows);

	function linksHref(row: ArchiveViewRow): string {
		return linksFileHref(contentId, { item: row.item.id }, data.filters);
	}

	// 選択と公開の切り替え。
	let selectedIds = $state<number[]>([]);
	// 絞り込みで見えなくなった行は、選択から外す。見えない行まで一括で切り替えないため。
	$effect(() => {
		const visible = new Set(visibleRows.map((row) => row.item.id));
		const kept = selectedIds.filter((id) => visible.has(id));
		if (kept.length !== selectedIds.length) selectedIds = kept;
	});
	let allSelected = $derived(
		visibleRows.length > 0 && visibleRows.every((row) => selectedIds.includes(row.item.id))
	);

	function toggleSelectAll(checked: boolean) {
		selectedIds = checked ? visibleRows.map((row) => row.item.id) : [];
	}

	function toggleSelect(itemId: number, checked: boolean) {
		selectedIds = checked ? [...selectedIds, itemId] : selectedIds.filter((id) => id !== itemId);
	}

	let savingIds = $state<number[]>([]);
	/** 一括で切り替え中の公開状態。スピナーは押した方のボタンにだけ出す。 */
	let bulkTarget = $state<boolean | null>(null);
	let rescanning = $state(false);
	let rescanMessage = $state<string | null>(null);
	/** 走っているスキャンの進み具合。押したボタンの横に出す (→ docs/archive.md「スキャン」)。 */
	let scanProgress = $state<string | undefined>();
	let scanning = $derived(rescanning || scanProgress !== undefined);
	let anyBusy = $derived(savingIds.length > 0 || bulkTarget !== null || scanning);

	// 開いたときにスキャンが走っていれば、終わるまで押させずに進み具合を出し、終わったら一覧を取り直す。
	// スキャンは画面を閉じても続くので、開き直した画面がそれを知らないと、押し直して 409 になるため。
	$effect(() => {
		if (!editable) return;
		const controller = new AbortController();
		waitForRunningScan(contentId, (message) => (scanProgress = message), controller.signal).then(
			(finished) => {
				if (finished) invalidateAll();
			}
		);
		return () => controller.abort();
	});

	async function setPublished(itemId: number, published: boolean) {
		if (anyBusy) return;
		savingIds = [...savingIds, itemId];
		try {
			// 見出しの件数も変わるので、一覧ごと取り直す。
			await errorDialog.attempt(
				() =>
					client.PUT('/api/v1/contents/{id}/items/{item_id}', {
						params: { path: { id: contentId, item_id: itemId } },
						body: { published }
					}),
				() => invalidateAll()
			);
		} finally {
			savingIds = savingIds.filter((id) => id !== itemId);
		}
	}

	async function setBulkPublished(published: boolean) {
		if (anyBusy || selectedIds.length === 0) return;
		bulkTarget = published;
		const itemIds = [...selectedIds];
		try {
			await errorDialog.attempt(
				() =>
					client.PUT('/api/v1/contents/{id}/items', {
						params: { path: { id: contentId } },
						body: { itemIds, published }
					}),
				async () => {
					selectedIds = [];
					await invalidateAll();
				}
			);
		} finally {
			bulkTarget = null;
		}
	}

	// 再スキャン。結果はボタンの近くに次の再スキャンまで残し、一覧を取り直す (増減があるため)。
	async function rescan() {
		if (anyBusy) return;
		rescanning = true;
		// 失敗したときに、前回の成功の件数を今回の結果と読み違えさせない。
		rescanMessage = null;
		try {
			const scan = await rescanArchive(contentId, (message) => (scanProgress = message));
			if (!scan.ok) {
				errorDialog.show(scan.message);
				return;
			}
			rescanMessage = m.archive_items_rescan_result({
				added: formatNumber(scan.result.added),
				removed: formatNumber(scan.result.removed)
			});
			selectedIds = [];
			await invalidateAll();
		} finally {
			rescanning = false;
		}
	}

	/** 選択のチェック。タイルでは左上の隅に重ね、リストでは行の頭に置く。どちらも行いっぱいのリンクより上で押せるようにする。 */
	let cornerClass = $derived(
		browseLayout.tile ? 'absolute top-3 left-3 z-10' : browseRowAboveClass
	);
	/** タイルでは、選択のチェックと向かい合う右上の隅に置く。 */
	let switchClass = $derived(
		browseLayout.tile ? 'absolute top-2.5 right-3 z-10' : browseRowAboveClass
	);
</script>

{#snippet selectBox(row: ArchiveViewRow)}
	{#if editable}
		<Checkbox
			class={cornerClass}
			bind:checked={
				() => selectedIds.includes(row.item.id), (checked) => toggleSelect(row.item.id, checked)
			}
			disabled={anyBusy}
			aria-label={m.archive_items_select_row_label({ path: row.item.relPath })}
		/>
	{/if}
{/snippet}

{#snippet publishSwitch(row: ArchiveViewRow)}
	{@const item = byId.get(row.item.id)}
	{#if item}
		<Switch
			class={switchClass}
			bind:checked={() => item.published, (checked) => setPublished(item.id, checked)}
			disabled={anyBusy || !editable}
			aria-label={m.archive_items_publish_row_label({ path: item.relPath })}
		/>
	{/if}
{/snippet}

<!-- 閲覧の画面と同じ軸の値に、パスを足す。見るだけのときも、公開の状態は押せない切り替えで見せる。 -->
{#snippet itemSubtitle(row: ArchiveViewRow)}
	<ListRowSubtitle
		text={[row.item.subtitle ?? '', row.item.relPath].filter((part) => part !== '')}
	/>
{/snippet}

{#snippet statusFilter()}
	<Select.Root type="single" bind:value={status}>
		<BrowseSelectTrigger
			label={m.archive_items_status_label()}
			value={STATUS_LABELS[status]()}
			highlighted={status !== 'all'}
		/>
		<Select.Content>
			{#each ['all', 'published', 'unpublished'] as const as value (value)}
				<Select.Item {value}>
					<span>{STATUS_LABELS[value]()}</span>
					<span class="text-xs text-muted-foreground">{statusCount(value)}</span>
				</Select.Item>
			{/each}
		</Select.Content>
	</Select.Root>
{/snippet}

<div class={adminPageClass}>
	<div class="flex flex-col gap-4">
		{#if editable}
			<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
				<LoadingButton variant="outline" loading={scanning} disabled={anyBusy} onclick={rescan}>
					<RefreshCwIcon class="size-4" />
					{m.archive_items_rescan_button()}
				</LoadingButton>
				{#if scanProgress}
					<p role="status" class="text-sm text-muted-foreground">{scanProgress}</p>
				{:else if rescanMessage}
					<p class="text-sm text-muted-foreground">{rescanMessage}</p>
				{/if}
			</div>
		{/if}

		{#if view.folderMissing && view.items.length > 0}
			<WarningBand class="rounded-md border">
				<p class="text-sm leading-relaxed">{m.error_detail_archive_folder_missing()}</p>
			</WarningBand>
		{/if}

		<div>
			<ArchiveViewControls
				axes={view.axes}
				filters={data.filters}
				sort={data.sort}
				path={resolve('/admin/contents/[id]/items', { id: String(contentId) })}
				total={view.items.length}
				shown={listFilter.active || status !== 'all' ? visibleRows.length : undefined}
				countOnlyWhenFiltered
				hasItems={view.items.length > 0}
				folderMissing={view.folderMissing}
				extra={view.items.length > 0 ? statusFilter : undefined}
			/>
		</div>
	</div>

	{#if editable && visibleRows.length > 0}
		<!-- 選択中は画面の上端に留める。件数の多いアーカイブで全選択しても操作を見失わないため。
		     留めたときに下の行が透けないよう塗るが、色は地 (ground) のままにして、選んだだけで段の色が変わらないようにする。 -->
		<div
			class={[
				'-mx-4 flex min-h-14 flex-wrap items-center gap-x-4 gap-y-2 px-4 py-2 md:-mx-6 md:px-6',
				selectedIds.length > 0 && 'sticky top-0 z-20 border-b bg-ground'
			]}
		>
			<!-- 見せる文言は選んだ件数に変わるが、チェックの名前は aria-label で「すべて選択」に保つ。
			     文言が名前と同じときは、読み上げが2回にならないよう隠す。 -->
			<div class="flex items-center gap-2 text-sm">
				<Checkbox
					id="select-all-items"
					bind:checked={() => allSelected, toggleSelectAll}
					indeterminate={selectedIds.length > 0 && !allSelected}
					disabled={anyBusy}
					aria-label={m.archive_items_select_all_label()}
				/>
				<label for="select-all-items" aria-hidden={selectedIds.length === 0}>
					{selectedIds.length > 0
						? m.archive_items_selected_count({ count: formatNumber(selectedIds.length) })
						: m.archive_items_select_all_label()}
				</label>
			</div>
			{#if selectedIds.length > 0}
				<!-- 公開と非公開は向きが逆なだけで重みは同じなので、どちらかを主のボタンにしない。 -->
				<div class="flex flex-wrap gap-2">
					<LoadingButton
						size="sm"
						variant="outline"
						loading={bulkTarget === true}
						disabled={anyBusy}
						onclick={() => setBulkPublished(true)}
					>
						{m.marks_publish_button()}
					</LoadingButton>
					<LoadingButton
						size="sm"
						variant="outline"
						loading={bulkTarget === false}
						disabled={anyBusy}
						onclick={() => setBulkPublished(false)}
					>
						{m.marks_unpublish_button()}
					</LoadingButton>
					<Button size="sm" variant="ghost" disabled={anyBusy} onclick={() => (selectedIds = [])}>
						{m.action_cancel()}
					</Button>
				</div>
			{/if}
		</div>
	{/if}

	<div class="-mx-4 md:-mx-6">
		{#if view.items.length === 0}
			<p class={pageEmptyTextClass}>
				{view.folderMissing ? m.error_detail_archive_folder_missing() : m.archive_items_empty()}
			</p>
		{:else if rows.length === 0}
			<p class={pageEmptyTextClass}>{m.archive_view_empty()}</p>
		{:else}
			<ArchiveViewList
				{rows}
				{linksHref}
				urls={MANAGE_URLS}
				{matches}
				leading={selectBox}
				trailing={publishSwitch}
				subtitle={itemSubtitle}
			/>
		{/if}
	</div>
</div>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
