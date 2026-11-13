<script lang="ts">
	import { refreshAll } from '$app/navigation';
	import { resolve } from '$app/paths';
	import * as m from '#lib/paraglide/messages.js';
	import { breadcrumbLinkClass, pathCrumbs } from '#lib/breadcrumb.ts';
	import { SORT_OPTIONS, browseSortHref, type BrowseSort } from '#lib/browse-sort.ts';
	import { ListFilterState, provideListFilter } from '#lib/list-filter.svelte.ts';
	import { browseGutterClass, rowMenuSlotClass } from '#lib/list-row.ts';
	import { pageEmptyTextClass, pageHeadingInRowClass } from '#lib/page-layout.ts';
	import { pageTitle } from '#lib/page-title.ts';
	import { ShareOpsState } from '#lib/share-ops-state.svelte.ts';
	import { canAddTo, childSharePath, shareRowActions, type ShareDir } from '#lib/share-ops.ts';
	import BrowseBreadcrumb from '#lib/components/browse-breadcrumb.svelte';
	import BrowseControls from '#lib/components/browse-controls.svelte';
	import BrowseSortSelect from '#lib/components/browse-sort.svelte';
	import FolderEntryList, { type FolderEntryRow } from '#lib/components/folder-entry-list.svelte';
	import RowActionsMenu from '#lib/components/row-actions-menu.svelte';
	import ShareAddMenu from '#lib/components/share-ops/share-add-menu.svelte';
	import ShareOpsHost from '#lib/components/share-ops/share-ops-host.svelte';
	import ShareRowMenuItems from '#lib/components/share-ops/share-row-menu-items.svelte';
	import type { PageProps } from './$types';

	// 共有の中の画面 (→ docs/ui.md「共有の中の画面」)。フォルダーのコンテンツの画面と同じ一覧に、
	// その人に許された操作 (追加・⋮) を重ねる。
	let { data }: PageProps = $props();

	const listFilter = new ListFilterState();
	provideListFilter(listFilter);

	let browse = $derived(data.browse);
	let shareId = $derived(data.shareId);
	let crumbs = $derived(pathCrumbs(browse.path));
	let currentName = $derived(crumbs.at(-1)?.name ?? browse.share.name);

	let dir = $derived<ShareDir>({
		shareId,
		shareName: browse.share.name,
		path: browse.path,
		access: browse.access,
		trash: browse.trash,
		uploadMaxBytes: browse.uploadMaxBytes ?? null
	});
	const ops = new ShareOpsState(
		() => dir,
		() => data.shares,
		() => refreshAll()
	);

	// 並び順は階層を移っても保つ (フォルダーの画面と同じ)。
	function browseHref(path: string): string {
		return browseSortHref(
			resolve('/shares/[id]', { id: String(shareId) }),
			data.sort,
			path === '' ? {} : { path }
		);
	}

	function sortHref(sort: BrowseSort): string {
		return browseSortHref(resolve('/shares/[id]', { id: String(shareId) }), sort, {
			...(browse.path === '' ? {} : { path: browse.path }),
			...listFilter.query
		});
	}

	let rows = $derived<FolderEntryRow[]>(
		browse.entries.map((entry) => ({
			source: { kind: 'share', id: shareId },
			path: childSharePath(browse.path, entry.name),
			entry,
			badge: entry.listedAs ? m.shares_listed_badge() : undefined
		}))
	);

	let listedByPath = $derived(
		new Map(
			browse.entries.map((entry) => [childSharePath(browse.path, entry.name), entry.listedAs])
		)
	);

	function targetOf(row: FolderEntryRow) {
		return {
			name: row.entry.name,
			path: row.path,
			isDir: row.entry.isDir,
			listed: listedByPath.get(row.path) != null
		};
	}

	function hasActions(row: FolderEntryRow): boolean {
		return shareRowActions(browse.access, targetOf(row), ops.copyDestinationExists).length > 0;
	}
</script>

{#snippet rowMenu(row: FolderEntryRow)}
	{#if hasActions(row)}
		<span class={rowMenuSlotClass()}>
			<RowActionsMenu name={row.entry.name}
				><ShareRowMenuItems {ops} target={targetOf(row)} /></RowActionsMenu
			>
		</span>
	{/if}
{/snippet}

<svelte:head><title>{pageTitle(currentName)}</title></svelte:head>

<div class="py-6">
	<div class={browseGutterClass}>
		<!-- ホーム / 共有 / 共有の名前 / … (→ docs/ui.md「共有の中の画面」)。 -->
		<BrowseBreadcrumb ancestors={[]}>
			<span aria-hidden="true">/</span>

			<a href={resolve('shares')} class={breadcrumbLinkClass}>{m.shares_heading()}</a>

			<span aria-hidden="true">/</span>
			{#if crumbs.length === 0}
				<span>{browse.share.name}</span>
			{:else}
				<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
				<a href={browseHref('')} class={breadcrumbLinkClass}>{browse.share.name}</a>
			{/if}
			{#each crumbs as crumb, index (crumb.path)}
				<span aria-hidden="true">/</span>
				{#if index === crumbs.length - 1}
					<span>{crumb.name}</span>
				{:else}
					<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
					<a href={browseHref(crumb.path)} class={breadcrumbLinkClass}>{crumb.name}</a>
				{/if}
			{/each}
		</BrowseBreadcrumb>

		<div class="mb-4 flex flex-col gap-1">
			<div class="flex items-center gap-2">
				<h1 class={pageHeadingInRowClass}>{currentName}</h1>
				{#if canAddTo(browse.access)}
					<ShareAddMenu {ops} />
				{/if}
			</div>
			<!-- ここを載せたコンテンツがあれば、ホームで何という名前で見えているかを出し、そのコンテンツへつなぐ。 -->
			{#if browse.listedAs}
				<a
					href={resolve('/folders/[id]', { id: String(browse.listedAs.contentId) })}
					class="relative self-start text-sm text-muted-foreground underline underline-offset-4 after:absolute after:inset-x-0 after:-inset-y-3"
				>
					{m.shares_listed_heading({ title: browse.listedAs.title })}
				</a>
			{/if}
		</div>

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
			linksHref={() => undefined}
			trailing={rowMenu}
		/>
	{/if}
</div>

<ShareOpsHost {ops} />
