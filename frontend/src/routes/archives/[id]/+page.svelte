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
	import type { PageProps } from './$types';
	import { pageTitle } from '$lib/page-title';
	import { pageEmptyTextClass } from '$lib/page-layout';
	import { client } from '$lib/api/client';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import ListRowHidden from '$lib/components/list-row-hidden.svelte';
	import RowActionsMenu from '$lib/components/row-actions-menu.svelte';

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

	/** 名前の絞り込みで残る行の数。一覧の部品と同じくタイトルで照らす。 */
	let shownCount = $derived(listFilter.apply(view.items, (item) => item.title).length);
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
		<ArchiveViewList
			{rows}
			{linksHref}
			trailing={view.canEdit ? rowMenu : undefined}
			isHidden={(row) => unpublished.includes(row.item.id)}
			{hiddenRow}
		/>
	{/if}
</div>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
