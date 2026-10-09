<script lang="ts">
	import { resolve } from '$app/paths';
	import { withQuery } from '$lib/href';
	import * as m from '$lib/paraglide/messages.js';
	import { linksFileHref } from '$lib/links-file';
	import { breadcrumbLinkClass, pathCrumbs } from '$lib/breadcrumb';
	import { SORT_OPTIONS, DEFAULT_BROWSE_SORT, type BrowseSort } from '$lib/browse-sort';
	import BrowseBreadcrumb from '$lib/components/browse-breadcrumb.svelte';
	import BrowseLayoutToggle from '$lib/components/browse-layout-toggle.svelte';
	import BrowseSortSelect from '$lib/components/browse-sort.svelte';
	import FolderEntryList, { type FolderEntryRow } from '$lib/components/folder-entry-list.svelte';
	import { browseControlsClass, browseGutterClass } from '$lib/list-row';
	import type { PageProps } from './$types';
	import { pageTitle } from '$lib/page-title';
	import { pageHeadingClass, pageEmptyTextClass } from '$lib/page-layout';

	let { data }: PageProps = $props();

	let browse = $derived(data.browse);
	let contentId = $derived(data.contentId);

	let breadcrumbs = $derived(pathCrumbs(browse.path));

	let currentName = $derived(breadcrumbs.at(-1)?.name ?? browse.folderTitle);

	// 並び順はパスを移動しても保つ (フォルダの中を辿る操作であり、選び直す理由が無いため)。
	function browseHref(path: string): string {
		return withQuery(resolve('/folders/[id]', { id: String(contentId) }), {
			...(path === '' ? {} : { path }),
			...(data.sort === DEFAULT_BROWSE_SORT ? {} : { sort: data.sort })
		});
	}

	function sortHref(sort: BrowseSort): string {
		return withQuery(resolve('/folders/[id]', { id: String(contentId) }), {
			...(browse.path === '' ? {} : { path: browse.path }),
			...(sort === DEFAULT_BROWSE_SORT ? {} : { sort })
		});
	}

	/** 表示中のパス直下にある `name` の、フォルダ内の相対パス。 */
	function childPath(name: string): string {
		return browse.path === '' ? name : `${browse.path}/${name}`;
	}

	let rows = $derived<FolderEntryRow[]>(
		browse.entries.map((entry) => ({ contentId, path: childPath(entry.name), entry }))
	);

	function linksHref(row: FolderEntryRow): string {
		return linksFileHref(
			contentId,
			{ path: row.path },
			data.sort === DEFAULT_BROWSE_SORT ? {} : { sort: data.sort }
		);
	}
</script>

<svelte:head><title>{pageTitle(currentName)}</title></svelte:head>

<div class="py-6">
	<div class={browseGutterClass}>
		<!-- パンくずは常にURLナビゲーション(aタグ)で遷移する。ブラウザの戻る/進むが
		     自然に機能するようにするため、ローカルstateだけでpathを書き換えない。
		     フォルダ直下でも出す (トップと親グループへ戻る導線がここにしかないため)。 -->
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

		<h1 class={pageHeadingClass}>{currentName}</h1>

		{#if browse.entries.length > 0}
			<div class={['mb-4', browseControlsClass]}>
				<BrowseSortSelect value={data.sort} options={SORT_OPTIONS} href={sortHref} />
				<BrowseLayoutToggle />
			</div>
		{/if}
	</div>

	{#if browse.entries.length === 0}
		<p class={pageEmptyTextClass}>{m.folder_browse_empty()}</p>
	{:else}
		<FolderEntryList {rows} dirHref={(row) => browseHref(row.path)} {linksHref} />
	{/if}
</div>
