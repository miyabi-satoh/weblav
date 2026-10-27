<script lang="ts">
	import { tick, untrack } from 'svelte';
	import { refreshAll } from '$app/navigation';
	import { resolve } from '$app/paths';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import { client } from '$lib/api/client';
	import {
		archiveItemManageDownloadHref,
		archiveItemManageThumbnailHref,
		archiveManageFileHref
	} from '$lib/api/urls';
	import type { components } from '$lib/api/schema';
	import { cancelScan, rescanArchive, waitForRunningScan } from '$lib/archive-scan';
	import ScanProgress from '$lib/components/scan-progress.svelte';
	import { browseLayout } from '$lib/browse-layout.svelte';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { formatNumber } from '$lib/format';
	import { linksFileHref } from '$lib/links-file';
	import { LIST_FILTER_QUERY } from '$lib/list-filter';
	import { ArchivePagedList } from '$lib/archive-paged-list.svelte';
	import { archiveListQuery, manageChunks, manageShuffle } from '$lib/archive-list-request';
	import ArchiveStaleNotice from '$lib/components/archive-stale-notice.svelte';
	import ListFilterEmpty from '$lib/components/list-filter-empty.svelte';
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
	type ManageArchiveViewResponse = components['schemas']['ManageArchiveViewResponse'];

	// アーカイブのアイテムのタブ (→ docs/ui.md「アーカイブのアイテムのタブ」)。閲覧の画面と同じ部品で並べ、
	// 選択と公開の切り替えを重ねる。見出し (タイトル・件数) とタブは [id]/+layout.svelte が出す。
	let { data }: PageProps = $props();

	const listFilter = new ListFilterState();
	provideListFilter(listFilter);

	const errorDialog = new ErrorDialogState();

	let contentId = $derived(data.contentId);
	let editable = $derived(data.editable);

	/** 未公開も開ける管理用の口。 */
	const MANAGE_URLS: ArchiveItemUrls = {
		download: archiveItemManageDownloadHref,
		thumbnail: archiveItemManageThumbnailHref,
		file: archiveManageFileHref
	};

	type Status = 'all' | 'published' | 'unpublished';
	/** 公開の状態での絞り込み。URL には持たない (閲覧者に見える姿を確かめる画面の都合で、共有するものではないため)。 */
	let status = $state<Status>('all');
	const STATUS_LABELS: Record<Status, () => string> = {
		all: m.archive_view_filter_all,
		published: m.archive_items_status_published,
		unpublished: m.archive_items_status_unpublished
	};

	/** 一覧に効いている名前の語と公開の状態。名前の語は打ち止めて 0.3 秒後に送る (→ docs/ui.md「一覧の絞り込み」)。 */
	let appliedName = $derived(data.nameFilter);
	let appliedStatus = $state<Status>('all');

	/**
	 * 名前の語・公開の状態で取り直した結果。ページの読み込みは画面の URL の語しか知らないので
	 * (shallow routing は読み込みを走らせない)、それと違う語・状態のときはこちらを出す。
	 * 軸・並び順を選び直す (ページを移る) と `data` が替わるので、取ったときの `data` も持って見分ける。
	 */
	let refetched = $state.raw<{
		data: typeof data;
		name: string;
		status: Status;
		view: ManageArchiveViewResponse;
	} | null>(null);
	let fromData = $derived(appliedName === data.nameFilter && appliedStatus === 'all');
	let view = $derived(
		!fromData &&
			refetched?.data === data &&
			refetched.name === appliedName &&
			refetched.status === appliedStatus
			? refetched.view
			: data.view
	);

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

	let listView = $state<ReturnType<typeof ArchiveViewList<ManageItem>> | undefined>();

	/** 今の軸・並び順・名前の語・公開の状態で、最初の区切りから取り直す。 */
	async function refetch(keepPosition: boolean) {
		const position = keepPosition ? (listView?.firstVisibleIndex() ?? 0) : 0;
		const requestedFor = data;
		const name = appliedName;
		const wanted = appliedStatus;
		// 軸名がキーのクエリは生成された型に載らない (→ archives/[id]/+page.ts)。
		const requestInit = {
			params: {
				path: { id: contentId },
				query: archiveListQuery(data.filters, {
					sort: data.sort,
					[LIST_FILTER_QUERY]: name,
					status: wanted === 'all' ? '' : wanted
				})
			}
		} as unknown as { params: { path: { id: number } } };
		// 取れなかったら理由を出す。黙ると、空の行が出たまま再読み込みのボタンも効かないように見えるため。
		const result = await errorDialog.attempt(() =>
			client.GET('/api/v1/contents/{id}/manage-archive', requestInit)
		);
		const next = result?.data;
		// 待つ間にページを移った・別の語・状態を選んだなら、古い結果を出さない。
		if (!next || requestedFor !== data || name !== appliedName || wanted !== appliedStatus) return;
		refetched = { data, name, status: wanted, view: next };
		if (position > 0) void listView?.scrollToIndex(position);
	}

	$effect(() => {
		const typed = listFilter.value.trim();
		if (typed === appliedName.trim()) return;
		const timer = setTimeout(() => {
			appliedName = typed;
			if (!fromData) void refetch(false);
		}, 300);
		return () => clearTimeout(timer);
	});

	$effect(() => {
		if (status === appliedStatus) return;
		appliedStatus = status;
		if (!fromData) void refetch(false);
	});

	// 公開の状態で絞ったまま軸・並び順を選び直したら、新しいページの読み込み (状態は「すべて」) に状態を効かせて取り直す。
	$effect(() => {
		const current = data;
		untrack(() => {
			if (!reloading && !fromData && refetched?.data !== current) void refetch(false);
		});
	});

	// 結果が捨てられた (しばらく使わなかった) ら、見ていた所から取り直す。
	$effect(() => {
		if (list.expired) {
			if (fromData) void refreshAll();
			else void refetch(true);
		}
	});

	/** 書き換えた後に、見出しの件数ごと取り直す。読み込みは画面の URL の語で取るので、違う語・状態なら続けて取り直す。 */
	// 取り直している間は、上の `$effect` に取り直させない。ここで見ていた所から取り直すため。
	let reloading = false;
	async function reload() {
		reloading = true;
		try {
			await refreshAll();
			if (!fromData) await refetch(true);
		} finally {
			reloading = false;
		}
	}

	function linksHref(row: ArchiveViewRow<ManageItem>): string {
		return linksFileHref(contentId, { item: row.item.id }, data.filters);
	}

	// 選択と公開の切り替え (→ docs/archive.md「まとめて公開を切り替える」)。
	// 「すべて選択」は、絞り込みに合う全部 (サーバーが持つ結果の集まり) を選び、外した行を `exceptIds` に持つ。
	// 手元に全件が無いので、番号を全部集められないため。
	let selectAll = $state(false);
	let selectedIds = $state<number[]>([]);
	let exceptIds = $state<number[]>([]);
	// 一覧を取り直したら、選択は外す。選択は前の結果の番号で持っていて、新しい結果に引き継ぐと、
	// 数も対象も変わったのに気づけないため。
	// ただし1行の公開を切り替えて取り直すときは残す。公開の状態で絞っていなければ、結果の行は変わらないため。
	let keepSelection = false;
	$effect(() => {
		void list;
		if (keepSelection) return;
		selectAll = false;
		selectedIds = [];
		exceptIds = [];
	});
	let selectedCount = $derived(
		selectAll ? Math.max(0, list.total - exceptIds.length) : selectedIds.length
	);
	let allSelected = $derived(selectAll && exceptIds.length === 0 && list.total > 0);

	let nameOrStatusActive = $derived(appliedName.trim() !== '' || appliedStatus !== 'all');

	function isSelected(itemId: number): boolean {
		return selectAll ? !exceptIds.includes(itemId) : selectedIds.includes(itemId);
	}

	function toggleSelectAll(checked: boolean) {
		selectAll = checked;
		selectedIds = [];
		exceptIds = [];
	}

	function toggleSelect(itemId: number, checked: boolean) {
		if (selectAll) {
			exceptIds = checked ? exceptIds.filter((id) => id !== itemId) : [...exceptIds, itemId];
		} else {
			selectedIds = checked ? [...selectedIds, itemId] : selectedIds.filter((id) => id !== itemId);
		}
	}

	function clearSelection() {
		toggleSelectAll(false);
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
				if (finished) void reload();
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
				async () => {
					keepSelection = appliedStatus === 'all';
					try {
						await reload();
						// 取り直した一覧の後始末 (上の `$effect`) が走り終えるまで残す。
						await tick();
					} finally {
						keepSelection = false;
					}
				}
			);
		} finally {
			savingIds = savingIds.filter((id) => id !== itemId);
		}
	}

	async function setBulkPublished(published: boolean) {
		if (anyBusy || selectedCount === 0) return;
		bulkTarget = published;
		// すべて選択したときは、画面が見ていた結果の集まりの名前と外した行を送る。番号を全部集められないため。
		const body = selectAll
			? { snapshot: list.snapshot, exceptIds: [...exceptIds], published }
			: { itemIds: [...selectedIds], published };
		try {
			await errorDialog.attempt(
				() =>
					client.PUT('/api/v1/contents/{id}/items', {
						params: { path: { id: contentId } },
						body
					}),
				async () => {
					clearSelection();
					await reload();
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
			rescanMessage = scan.result.cancelled
				? m.archive_items_rescan_cancelled()
				: m.archive_items_rescan_result({
						added: formatNumber(scan.result.added),
						removed: formatNumber(scan.result.removed)
					});
			clearSelection();
			await reload();
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

{#snippet selectBox(row: ArchiveViewRow<ManageItem>)}
	{#if editable}
		<Checkbox
			class={cornerClass}
			bind:checked={() => isSelected(row.item.id), (checked) => toggleSelect(row.item.id, checked)}
			disabled={anyBusy}
			aria-label={m.archive_items_select_row_label({ path: row.item.relPath })}
		/>
	{/if}
{/snippet}

{#snippet publishSwitch(row: ArchiveViewRow<ManageItem>)}
	{@const item = row.item}
	<Switch
		class={switchClass}
		bind:checked={() => item.published, (checked) => setPublished(item.id, checked)}
		disabled={anyBusy || !editable}
		aria-label={m.archive_items_publish_row_label({ path: item.relPath })}
	/>
{/snippet}

<!-- 閲覧の画面と同じ軸の値に、パスを足す。見るだけのときも、公開の状態は押せない切り替えで見せる。 -->
{#snippet itemSubtitle(row: ArchiveViewRow<ManageItem>)}
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
					<span class="text-xs text-muted-foreground">{view.statusCounts[value]}</span>
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
					<ScanProgress message={scanProgress} oncancel={() => cancelScan(contentId)} />
				{:else if rescanMessage}
					<p class="text-sm text-muted-foreground">{rescanMessage}</p>
				{/if}
			</div>
		{/if}

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
				path={resolve('/admin/contents/[id]/items', { id: String(contentId) })}
				total={nameOrStatusActive ? view.totalAll : view.total}
				shown={nameOrStatusActive ? view.total : undefined}
				countOnlyWhenFiltered
				hasItems={view.totalAll > 0}
				folderMissing={view.folderMissing}
				extra={view.totalAll > 0 ? statusFilter : undefined}
			/>
			{#if list.stale}
				<ArchiveStaleNotice onreload={() => void reload()} />
			{/if}
		</div>
	</div>

	{#if editable && view.total > 0}
		<!-- 選択中は画面の上端に留める。件数の多いアーカイブで全選択しても操作を見失わないため。
		     留めたときに下の行が透けないよう塗るが、色は地 (ground) のままにして、選んだだけで段の色が変わらないようにする。 -->
		<div
			class={[
				'-mx-4 flex min-h-14 flex-wrap items-center gap-x-4 gap-y-2 px-4 py-2 md:-mx-6 md:px-6',
				selectedCount > 0 && 'sticky top-0 z-20 border-b bg-ground'
			]}
		>
			<!-- 見せる文言は選んだ件数に変わるが、チェックの名前は aria-label で「すべて選択」に保つ。
			     文言が名前と同じときは、読み上げが2回にならないよう隠す。 -->
			<div class="flex items-center gap-2 text-sm">
				<Checkbox
					id="select-all-items"
					bind:checked={() => allSelected, toggleSelectAll}
					indeterminate={selectedCount > 0 && !allSelected}
					disabled={anyBusy}
					aria-label={m.archive_items_select_all_label()}
				/>
				<label for="select-all-items" aria-hidden={selectedCount === 0}>
					{selectedCount > 0
						? m.archive_items_selected_count({ count: formatNumber(selectedCount) })
						: m.archive_items_select_all_label()}
				</label>
			</div>
			{#if selectedCount > 0}
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
					<Button size="sm" variant="ghost" disabled={anyBusy} onclick={clearSelection}>
						{m.action_cancel()}
					</Button>
				</div>
			{/if}
		</div>
	{/if}

	<div class="-mx-4 md:-mx-6">
		{#if view.totalAll === 0}
			<p class={pageEmptyTextClass}>
				{view.folderMissing ? m.error_detail_archive_folder_missing() : m.archive_items_empty()}
			</p>
		{:else if view.total === 0}
			{#if appliedName.trim() !== ''}
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
				leading={selectBox}
				trailing={publishSwitch}
				subtitle={itemSubtitle}
			/>
		{/if}
	</div>
</div>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
