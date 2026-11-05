<script lang="ts">
	import { untrack } from 'svelte';
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
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { formatNumber } from '$lib/format';
	import { linksFileHref } from '$lib/links-file';
	import { LIST_FILTER_QUERY } from '$lib/list-filter';
	import { ArchivePagedList } from '$lib/archive-paged-list.svelte';
	import { archiveListQuery, manageChunks, manageShuffle } from '$lib/archive-list-request';
	import ArchiveStaleNotice from '$lib/components/archive-stale-notice.svelte';
	import ListFilterEmpty from '$lib/components/list-filter-empty.svelte';
	import { ListFilterState, provideListFilter } from '$lib/list-filter.svelte';
	import { adminPageClass, pageEmptyTextClass } from '$lib/page-layout';
	import * as m from '$lib/paraglide/messages.js';
	import ArchiveViewControls from '$lib/components/archive-view-controls.svelte';
	import ArchiveViewList, {
		type ArchiveItemUrls,
		type ArchiveViewRow
	} from '$lib/components/archive-view-list.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import ListRowSubtitle from '$lib/components/list-row-subtitle.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import WarningBand from '$lib/components/warning-band.svelte';
	import type { PageProps } from './$types';

	type ManageItem = components['schemas']['ArchiveViewItem'];
	type ManageArchiveViewResponse = components['schemas']['ManageArchiveViewResponse'];

	// アーカイブのアイテムのタブ (→ docs/ui.md「アーカイブのアイテムのタブ」)。閲覧の画面と同じ部品で並べ、
	// 再スキャンとパスの表示を重ねる。見出し (タイトル・件数) とタブは [id]/+layout.svelte が出す。
	let { data }: PageProps = $props();

	const listFilter = new ListFilterState();
	provideListFilter(listFilter);

	const errorDialog = new ErrorDialogState();

	let contentId = $derived(data.contentId);
	let editable = $derived(data.editable);

	/** 管理の権限で開く口。 */
	const MANAGE_URLS: ArchiveItemUrls = {
		download: archiveItemManageDownloadHref,
		thumbnail: archiveItemManageThumbnailHref,
		file: archiveManageFileHref
	};

	/** 一覧に効いている名前の語。打ち止めて 0.3 秒後に送る (→ docs/ui.md「一覧の絞り込み」)。 */
	let appliedName = $derived(data.nameFilter);

	/**
	 * 名前の語で取り直した結果。ページの読み込みは画面の URL の語しか知らないので
	 * (shallow routing は読み込みを走らせない)、それと違う語のときはこちらを出す。
	 * 軸・並び順を選び直す (ページを移る) と `data` が替わるので、取ったときの `data` も持って見分ける。
	 */
	let refetched = $state.raw<{
		data: typeof data;
		name: string;
		view: ManageArchiveViewResponse;
	} | null>(null);
	let fromData = $derived(appliedName === data.nameFilter);
	let view = $derived(
		!fromData && refetched?.data === data && refetched.name === appliedName
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

	/** 今の軸・並び順・名前の語で、最初の区切りから取り直す。 */
	async function refetch(keepPosition: boolean) {
		const position = keepPosition ? (listView?.firstVisibleIndex() ?? 0) : 0;
		const requestedFor = data;
		const name = appliedName;
		// 軸名がキーのクエリは生成された型に載らない (→ archives/[id]/+page.ts)。
		const requestInit = {
			params: {
				path: { id: contentId },
				query: archiveListQuery(data.filters, {
					sort: data.sort,
					[LIST_FILTER_QUERY]: name
				})
			}
		} as unknown as { params: { path: { id: number } } };
		// 取れなかったら理由を出す。黙ると、空の行が出たまま再読み込みのボタンも効かないように見えるため。
		const result = await errorDialog.attempt(() =>
			client.GET('/api/v1/contents/{id}/manage-archive', requestInit)
		);
		const next = result?.data;
		// 待つ間にページを移った・別の語を打ったなら、古い結果を出さない。
		if (!next || requestedFor !== data || name !== appliedName) return;
		refetched = { data, name, view: next };
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

	// 名前で絞ったまま軸・並び順を選び直したら、新しいページの読み込みに名前の語を効かせて取り直す。
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

	/** 書き換えた後に、見出しの件数ごと取り直す。読み込みは画面の URL の語で取るので、違う語なら続けて取り直す。 */
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

	let rescanning = $state(false);
	let rescanMessage = $state<string | null>(null);
	/** 走っているスキャンの進み具合。押したボタンの横に出す (→ docs/archive.md「スキャン」)。 */
	let scanProgress = $state<string | undefined>();
	let scanning = $derived(rescanning || scanProgress !== undefined);
	let nameActive = $derived(appliedName.trim() !== '');

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

	// 再スキャン。結果はボタンの近くに次の再スキャンまで残し、一覧を取り直す (増減があるため)。
	async function rescan() {
		if (scanning) return;
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
			await reload();
		} finally {
			rescanning = false;
		}
	}
</script>

<!-- 閲覧の画面と同じ軸の値に、パスを足す。 -->
{#snippet itemSubtitle(row: ArchiveViewRow<ManageItem>)}
	<ListRowSubtitle
		text={[row.item.subtitle ?? '', row.item.relPath].filter((part) => part !== '')}
	/>
{/snippet}

<div class={adminPageClass}>
	<div class="flex flex-col gap-4">
		{#if editable}
			<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
				<LoadingButton variant="outline" loading={scanning} disabled={scanning} onclick={rescan}>
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
				total={nameActive ? view.totalAll : view.total}
				shown={nameActive ? view.total : undefined}
				countOnlyWhenFiltered
				hasItems={view.totalAll > 0}
				folderMissing={view.folderMissing}
			/>
			{#if list.stale}
				<ArchiveStaleNotice onreload={() => void reload()} />
			{/if}
		</div>
	</div>

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
				subtitle={itemSubtitle}
			/>
		{/if}
	</div>
</div>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
