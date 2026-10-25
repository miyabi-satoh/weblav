<script lang="ts">
	import FolderViewSwitch from '$lib/components/folder-view-switch.svelte';
	import BrowseHeading from '$lib/components/browse-heading.svelte';
	import { ListFilterState, provideListFilter } from '$lib/list-filter.svelte';
	import { resolve } from '$app/paths';
	import * as m from '$lib/paraglide/messages.js';
	import { linksFileHref } from '$lib/links-file';
	import { browseGutterClass, rowMenuSlotClass } from '$lib/list-row';
	import BrowseBreadcrumb from '$lib/components/browse-breadcrumb.svelte';
	import ArchiveViewControls from '$lib/components/archive-view-controls.svelte';
	import ArchiveViewList, { type ArchiveViewRow } from '$lib/components/archive-view-list.svelte';
	import ArchiveStaleNotice from '$lib/components/archive-stale-notice.svelte';
	import ListFilterEmpty from '$lib/components/list-filter-empty.svelte';
	import type { PageProps, Snapshot } from './$types';
	import { pageTitle } from '$lib/page-title';
	import { pageEmptyTextClass } from '$lib/page-layout';
	import { client } from '$lib/api/client';
	import type { components } from '$lib/api/schema';
	import { ArchivePagedList } from '$lib/archive-paged-list.svelte';
	import { archiveListQuery, viewChunks } from '$lib/archive-list-request';
	import { LIST_FILTER_QUERY } from '$lib/list-filter';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import ListRowHidden from '$lib/components/list-row-hidden.svelte';
	import RowActionsMenu from '$lib/components/row-actions-menu.svelte';

	type ArchiveViewResponse = components['schemas']['ArchiveViewResponse'];
	type ArchiveViewItem = components['schemas']['ArchiveViewItem'];

	let { data }: PageProps = $props();

	// ページ内の絞り込み (→ docs/ui.md「一覧の絞り込み」)。語はサーバーで照らし (→ docs/archive.md「一覧の API を区切って返す」)、
	// 強調だけを一覧の部品が context から読んで出す。
	const listFilter = new ListFilterState();
	provideListFilter(listFilter);

	let contentId = $derived(data.contentId);

	/** 名前の語を打ち直した・一覧が古くなって取り直した結果。ページを移ったら (軸・並び順を選び直したら) 捨てる。 */
	// `$state` だと中の `data` が包み直され、下の `refetched.data === data` が成り立たないので raw にする。
	let refetched = $state.raw<{ data: typeof data; view: ArchiveViewResponse } | null>(null);
	let view = $derived(refetched?.data === data ? refetched.view : data.view);

	let list = $derived(new ArchivePagedList<ArchiveViewItem>(view, viewChunks(data.contentId)));
	$effect(() => {
		const current = list;
		return () => current.dispose();
	});

	let listView = $state<ReturnType<typeof ArchiveViewList<ArchiveViewItem>> | undefined>();

	/** 今の軸・並び順・名前の語で、最初の区切りから取り直す。`keepPosition` なら、見ていた行へ戻る。 */
	async function refetch(nameFilter: string, keepPosition: boolean) {
		const position = keepPosition ? (listView?.firstVisibleIndex() ?? 0) : 0;
		const requestedFor = data;
		// 軸名がキーのクエリは生成された型に載らない (→ +page.ts)。
		const requestInit = {
			params: {
				path: { id: contentId },
				query: archiveListQuery(data.filters, {
					sort: data.sort,
					[LIST_FILTER_QUERY]: nameFilter
				})
			}
		} as unknown as { params: { path: { id: number } } };
		// 取れなかったら理由を出す。黙ると、空の行が出たまま再読み込みのボタンも効かないように見えるため。
		const result = await errorDialog.attempt(() =>
			client.GET('/api/v1/contents/{id}/archive', requestInit)
		);
		const next = result?.data;
		// 待つ間にページを移った・別の語を打ったなら、古い結果を出さない。
		if (!next || requestedFor !== data || nameFilter !== appliedName) return;
		refetched = { data, view: next };
		if (position > 0) void listView?.scrollToIndex(position);
	}

	/** 一覧に効いている名前の語。打ち止めて 0.3 秒後に送る (URL に写すのと同じ間。→ docs/ui.md「一覧の絞り込み」)。 */
	let appliedName = $derived(data.nameFilter);
	$effect(() => {
		const typed = listFilter.value.trim();
		if (typed === appliedName.trim()) return;
		const timer = setTimeout(() => {
			appliedName = typed;
			void refetch(typed, false);
		}, 300);
		return () => clearTimeout(timer);
	});

	// 結果が捨てられた (しばらく使わなかった) ら、見ていた所から取り直す。
	$effect(() => {
		if (list.expired) void refetch(appliedName, true);
	});

	function linksHref(row: ArchiveViewRow): string {
		return linksFileHref(contentId, { item: row.item.id }, data.filters);
	}

	// 書き換えられる人は、見ている画面から行を非公開にできる (→ docs/archive.md「アイテムの公開」)。
	// 押した行は、その場で「元に戻す」の行に替える。公開し直すのは、アイテムの管理画面でもできる。
	let unpublished = $state<number[]>([]);
	let busy = $state(false);
	const errorDialog = new ErrorDialogState();

	async function setPublished(row: ArchiveViewRow, published: boolean) {
		if (busy) return;
		busy = true;
		try {
			await errorDialog.attempt(
				() =>
					client.PUT('/api/v1/contents/{id}/items/{item_id}', {
						params: { path: { id: contentId, item_id: row.item.id } },
						body: { published }
					}),
				() => {
					unpublished = published
						? unpublished.filter((id) => id !== row.item.id)
						: [...unpublished, row.item.id];
				}
			);
		} finally {
			busy = false;
		}
	}

	let nameActive = $derived(appliedName.trim() !== '');

	// 戻る・進むで来たときは、見ていた行から出す (→ docs/archive.md「画面が見える辺りを取りに行く」)。
	export const snapshot: Snapshot<number> = {
		capture: () => listView?.firstVisibleIndex() ?? 0,
		restore: (index) => {
			if (index > 0) void listView?.scrollToIndex(index);
		}
	};
</script>

{#snippet rowMenu(row: ArchiveViewRow)}
	<span class={rowMenuSlotClass()}>
		<RowActionsMenu name={row.item.title}>
			<DropdownMenu.Item onSelect={() => void setPublished(row, false)}>
				{m.marks_unpublish_button()}
			</DropdownMenu.Item>
		</RowActionsMenu>
	</span>
{/snippet}

{#snippet hiddenRow(row: ArchiveViewRow)}
	<ListRowHidden
		message={m.archive_view_unpublished_row({ title: row.item.title })}
		{busy}
		onundo={() => void setPublished(row, true)}
	/>
{/snippet}

<svelte:head><title>{pageTitle(view.archiveTitle)}</title></svelte:head>

<div class="py-6">
	<div class={browseGutterClass}>
		<BrowseBreadcrumb ancestors={view.ancestors}>
			<span aria-hidden="true">/</span>
			<span>{view.archiveTitle}</span>
		</BrowseBreadcrumb>

		<BrowseHeading
			heading={view.archiveTitle}
			{contentId}
			contentTitle={view.archiveTitle}
			canEdit={view.canEdit}
		/>

		<div class="mb-4"><FolderViewSwitch {contentId} current="list" /></div>

		<ArchiveViewControls
			axes={view.axes}
			filters={data.filters}
			sort={data.sort}
			path={resolve('/archives/[id]', { id: String(contentId) })}
			total={nameActive ? view.totalAll : view.total}
			shown={nameActive ? view.total : undefined}
			hasItems={view.totalAll > 0}
			folderMissing={view.folderMissing}
		/>
		{#if list.stale}
			<ArchiveStaleNotice onreload={() => void refetch(appliedName, true)} />
		{/if}
	</div>

	{#if view.total === 0}
		{#if nameActive && view.totalAll > 0}
			<ListFilterEmpty />
		{:else}
			<p class={pageEmptyTextClass}>
				{view.folderMissing ? m.error_detail_archive_folder_missing() : m.archive_view_empty()}
			</p>
		{/if}
	{:else}
		<ArchiveViewList
			bind:this={listView}
			{list}
			archiveId={contentId}
			{linksHref}
			trailing={view.canEdit ? rowMenu : undefined}
			isHidden={(row) => unpublished.includes(row.item.id)}
			{hiddenRow}
		/>
	{/if}
</div>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
