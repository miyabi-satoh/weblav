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
	import ListRowHidden from '$lib/components/list-row-hidden.svelte';
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

	// 書き換えられる人は、行ごとに表示しないにできる (→ docs/folders.md「表示しないもの」)。
	// 押した行は、その場で「元に戻す」の行に替える。取り消しは編集ページの「表示しないもの」でもできる。
	type Hidden = { id: number; path: string; name: string };
	let hidden = $state<Hidden[]>([]);
	let busy = $state(false);
	const errorDialog = new ErrorDialogState();

	function hiddenOf(row: FolderEntryRow): Hidden | undefined {
		return hidden.find((entry) => entry.path === row.path);
	}

	async function hide(row: FolderEntryRow) {
		if (busy) return;
		busy = true;
		try {
			await errorDialog.attempt(
				() =>
					client.POST('/api/v1/contents/{id}/hidden-entries', {
						params: { path: { id: contentId } },
						body: { path: row.path }
					}),
				(result) => {
					if (!result.data) return;
					hidden.push({ id: result.data.id, path: row.path, name: row.entry.name });
				}
			);
		} finally {
			busy = false;
		}
	}

	async function undo(entry: Hidden) {
		if (busy) return;
		busy = true;
		try {
			await errorDialog.attempt(
				() =>
					client.DELETE('/api/v1/contents/{id}/hidden-entries/{entry_id}', {
						params: { path: { id: contentId, entry_id: entry.id } }
					}),
				() => {
					hidden = hidden.filter((h) => h !== entry);
				}
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
			<DropdownMenu.Item onSelect={() => void hide(row)}>
				{m.folder_browse_hide_menu()}
			</DropdownMenu.Item>
		</RowActionsMenu>
	</span>
{/snippet}

{#snippet hiddenRow(row: FolderEntryRow)}
	{@const entry = hiddenOf(row)}
	{#if entry}
		<ListRowHidden
			message={m.contents_hidden_row({ title: entry.name })}
			{busy}
			onundo={() => void undo(entry)}
		/>
	{/if}
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
			isHidden={(row) => hiddenOf(row) !== undefined}
			{hiddenRow}
		/>
	{/if}
</div>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
