<script lang="ts">
	import BrowseControls from '$lib/components/browse-controls.svelte';
	import BrowseHeading from '$lib/components/browse-heading.svelte';
	import { ListFilterState, provideListFilter } from '$lib/list-filter.svelte';
	import { resolve } from '$app/paths';
	import { withQuery } from '$lib/href';
	import * as m from '$lib/paraglide/messages.js';
	import { linksFileHref } from '$lib/links-file';
	import { breadcrumbLinkClass, pathCrumbs } from '$lib/breadcrumb';
	import { SORT_OPTIONS, DEFAULT_BROWSE_SORT, type BrowseSort } from '$lib/browse-sort';
	import BrowseBreadcrumb from '$lib/components/browse-breadcrumb.svelte';
	import BrowseSortSelect from '$lib/components/browse-sort.svelte';
	import FolderEntryList, { type FolderEntryRow } from '$lib/components/folder-entry-list.svelte';
	import { browseGutterClass, rowMenuSlotClass } from '$lib/list-row';
	import type { PageProps } from './$types';
	import { pageTitle } from '$lib/page-title';
	import { pageEmptyTextClass } from '$lib/page-layout';
	import { client } from '$lib/api/client';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import FolderViewSwitch from '$lib/components/folder-view-switch.svelte';
	import { invalidateAll } from '$app/navigation';
	import RowActionsMenu from '$lib/components/row-actions-menu.svelte';

	let { data }: PageProps = $props();

	// ページ内の絞り込み (→ docs/ui.md「一覧の絞り込み」)。一覧の部品が context から読む。
	const listFilter = new ListFilterState();
	provideListFilter(listFilter);

	let browse = $derived(data.browse);
	let contentId = $derived(data.contentId);

	let breadcrumbs = $derived(pathCrumbs(browse.path));

	let currentName = $derived(breadcrumbs.at(-1)?.name ?? browse.folderTitle);

	// 並び順はパスを移動しても保つ (フォルダーの中を辿る操作であり、選び直す理由が無いため)。
	function browseHref(path: string): string {
		return withQuery(resolve('/folders/[id]', { id: String(contentId) }), {
			...(path === '' ? {} : { path }),
			...(data.sort === DEFAULT_BROWSE_SORT ? {} : { sort: data.sort })
		});
	}

	function sortHref(sort: BrowseSort): string {
		return withQuery(resolve('/folders/[id]', { id: String(contentId) }), {
			...(browse.path === '' ? {} : { path: browse.path }),
			...(sort === DEFAULT_BROWSE_SORT ? {} : { sort }),
			// 並び順を変えても、打った絞り込みは残す。階層を移るとき (browseHref) は、別の一覧なので引き継がない。
			...listFilter.query
		});
	}

	/** 表示中のパス直下にある `name` の、フォルダー内の相対パス。 */
	function childPath(name: string): string {
		return browse.path === '' ? name : `${browse.path}/${name}`;
	}

	let rows = $derived<FolderEntryRow[]>(
		browse.entries.map((entry) => ({ contentId, path: childPath(entry.name), entry }))
	);

	// 書き換えられる人には、非公開のものも印を添えて並ぶ。行ごとに公開・非公開を切り替える
	// (→ docs/folders.md「公開と非公開」)。フォルダーの中で辿れるかが変わるので、一覧を取り直す。
	let busy = $state(false);
	const errorDialog = new ErrorDialogState();

	async function setPublic(row: FolderEntryRow, isPublic: boolean) {
		if (busy) return;
		busy = true;
		try {
			await errorDialog.attempt(
				() =>
					client.PUT('/api/v1/contents/{id}/marks', {
						params: { path: { id: contentId } },
						body: { path: row.path, public: isPublic }
					}),
				() => invalidateAll()
			);
		} finally {
			busy = false;
		}
	}

	function linksHref(row: FolderEntryRow): string {
		return linksFileHref(
			contentId,
			{ path: row.path },
			data.sort === DEFAULT_BROWSE_SORT ? {} : { sort: data.sort }
		);
	}
</script>

{#snippet rowMenu(row: FolderEntryRow)}
	<span class={rowMenuSlotClass()}>
		<RowActionsMenu name={row.entry.name}>
			{#if row.entry.public}
				<DropdownMenu.Item disabled={busy} onSelect={() => void setPublic(row, false)}>
					{m.marks_unpublish_button()}
				</DropdownMenu.Item>
			{:else}
				<DropdownMenu.Item disabled={busy} onSelect={() => void setPublic(row, true)}>
					{m.marks_publish_button()}
				</DropdownMenu.Item>
			{/if}
		</RowActionsMenu>
	</span>
{/snippet}

<svelte:head><title>{pageTitle(currentName)}</title></svelte:head>

<div class="py-6">
	<div class={browseGutterClass}>
		<!-- パンくずは常にURLナビゲーション(aタグ)で遷移する。ブラウザーの戻る/進むが
		     自然に機能するようにするため、ローカルstateだけでpathを書き換えない。
		     フォルダー直下でも出す (トップと親グループへ戻る導線がここにしかないため)。 -->
		<BrowseBreadcrumb ancestors={browse.ancestors}>
			<span aria-hidden="true">/</span>
			{#if breadcrumbs.length === 0}
				<span>{browse.folderTitle}</span>
			{:else}
				<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
				<a href={browseHref('')} class={breadcrumbLinkClass}>{browse.folderTitle}</a>
			{/if}
			{#each breadcrumbs as crumb, index (crumb.path)}
				<span aria-hidden="true">/</span>
				{#if index === breadcrumbs.length - 1}
					<span>{crumb.name}</span>
				{:else}
					<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
					<a href={browseHref(crumb.path)} class={breadcrumbLinkClass}>{crumb.name}</a>
				{/if}
			{/each}
		</BrowseBreadcrumb>

		<BrowseHeading
			heading={currentName}
			contentId={data.contentId}
			contentTitle={browse.folderTitle}
			canEdit={browse.canEdit}
		/>

		{#if browse.listView}
			<div class="mb-4"><FolderViewSwitch {contentId} current="folder" /></div>
		{/if}

		{#if browse.entries.length > 0}
			<BrowseControls class="mb-4">
				{#snippet sort()}
					<BrowseSortSelect value={data.sort} options={SORT_OPTIONS} href={sortHref} />
				{/snippet}
			</BrowseControls>
		{/if}
	</div>

	{#if browse.entries.length === 0}
		<p class={pageEmptyTextClass}>{m.folder_browse_empty()}</p>
	{:else}
		<FolderEntryList
			{rows}
			dirHref={(row) => browseHref(row.path)}
			{linksHref}
			trailing={browse.canEdit ? rowMenu : undefined}
		/>
	{/if}
</div>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
