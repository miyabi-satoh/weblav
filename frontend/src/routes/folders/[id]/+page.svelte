<script lang="ts">
	import BrowseControls from '$lib/components/browse-controls.svelte';
	import BrowseHeading from '$lib/components/browse-heading.svelte';
	import { ListFilterState, provideListFilter } from '$lib/list-filter.svelte';
	import { resolve } from '$app/paths';
	import * as m from '$lib/paraglide/messages.js';
	import { linksFileHref } from '$lib/links-file';
	import { breadcrumbLinkClass, pathCrumbs } from '$lib/breadcrumb';
	import { SORT_OPTIONS, browseSortHref, sortQuery, type BrowseSort } from '$lib/browse-sort';
	import BrowseBreadcrumb from '$lib/components/browse-breadcrumb.svelte';
	import BrowseSortSelect from '$lib/components/browse-sort.svelte';
	import FolderEntryList, { type FolderEntryRow } from '$lib/components/folder-entry-list.svelte';
	import { browseGutterClass, rowMenuSlotClass } from '$lib/list-row';
	import type { PageProps } from './$types';
	import { pageTitle } from '$lib/page-title';
	import { pageEmptyTextClass } from '$lib/page-layout';
	import FolderViewSwitch from '$lib/components/folder-view-switch.svelte';
	import { refreshAll } from '$app/navigation';
	import RowActionsMenu from '$lib/components/row-actions-menu.svelte';
	import { ShareOpsState } from '$lib/share-ops-state.svelte';
	import {
		canAddTo,
		childSharePath,
		shareRowActions,
		type ShareAction,
		type ShareDir,
		type ShareTarget
	} from '$lib/share-ops';
	import ShareAddMenu from '$lib/components/share-ops/share-add-menu.svelte';
	import ShareOpsHost from '$lib/components/share-ops/share-ops-host.svelte';
	import ShareRowMenuItems from '$lib/components/share-ops/share-row-menu-items.svelte';

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
		return browseSortHref(
			resolve('/folders/[id]', { id: String(contentId) }),
			data.sort,
			path === '' ? {} : { path }
		);
	}

	function sortHref(sort: BrowseSort): string {
		// 並び順を変えても、打った絞り込みは残す。階層を移るとき (browseHref) は、別の一覧なので引き継がない。
		return browseSortHref(resolve('/folders/[id]', { id: String(contentId) }), sort, {
			...(browse.path === '' ? {} : { path: browse.path }),
			...listFilter.query
		});
	}

	let rows = $derived<FolderEntryRow[]>(
		browse.entries.map((entry) => ({
			source: { kind: 'content', id: contentId },
			path: childSharePath(browse.path, entry.name),
			entry
		}))
	);

	// この場所が共有の中で操作できるなら、共有の画面と同じ操作を重ねる (→ docs/ui.md「共有の中の画面」)。
	let share = $derived(browse.share ?? null);
	let shareDir = $derived<ShareDir | null>(
		share === null
			? null
			: {
					shareId: share.id,
					shareName: share.name,
					path: share.path,
					access: share.access,
					trash: share.trash,
					// 上限はこの画面の応答に無いので、超えたものはサーバーが断る。
					uploadMaxBytes: null
				}
	);
	const shareOps = new ShareOpsState(
		() => shareDir!,
		() => data.shares,
		() => refreshAll()
	);
	// 中のフォルダーがホームに載っているかはこの画面の応答に無いので、「ホームに載せる」は共有の画面に任せる。
	const OMITTED_SHARE_ACTIONS: ShareAction[] = ['listOnHome'];

	function shareTarget(row: FolderEntryRow): ShareTarget | null {
		if (shareDir === null) return null;
		return {
			name: row.entry.name,
			path: childSharePath(shareDir.path, row.entry.name),
			isDir: row.entry.isDir,
			listed: false
		};
	}

	function hasShareActions(target: ShareTarget | null): boolean {
		if (target === null || shareDir === null) return false;
		return shareRowActions(shareDir.access, target, shareOps.copyDestinationExists).some(
			(action) => !OMITTED_SHARE_ACTIONS.includes(action)
		);
	}

	function linksHref(row: FolderEntryRow): string {
		return linksFileHref(contentId, { path: row.path }, sortQuery(data.sort));
	}
</script>

{#snippet rowMenu(row: FolderEntryRow)}
	{@const target = shareTarget(row)}
	{#if target !== null && hasShareActions(target)}
		<span class={rowMenuSlotClass()}>
			<RowActionsMenu name={row.entry.name}>
				<ShareRowMenuItems ops={shareOps} {target} omit={OMITTED_SHARE_ACTIONS} />
			</RowActionsMenu>
		</span>
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
		>
			{#snippet actions()}
				{#if shareDir !== null && canAddTo(shareDir.access)}
					<ShareAddMenu ops={shareOps} />
				{/if}
			{/snippet}
		</BrowseHeading>

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
			trailing={shareDir !== null ? rowMenu : undefined}
		/>
	{/if}
</div>

{#if shareDir !== null}
	<ShareOpsHost ops={shareOps} />
{/if}
