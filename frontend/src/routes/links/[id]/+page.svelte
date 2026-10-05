<script lang="ts">
	import { resolve } from '$app/paths';
	import { withQuery } from '$lib/href';
	import * as m from '$lib/paraglide/messages.js';
	import { archiveItemDownloadHref, contentDownloadHref } from '$lib/api/urls';
	import { breadcrumbLinkClass, pathCrumbs } from '$lib/breadcrumb';
	import { RefreshedLinkPreviews } from '$lib/link-previews.svelte';
	import BrowseBreadcrumb from '$lib/components/browse-breadcrumb.svelte';
	import BrowseLayoutToggle from '$lib/components/browse-layout-toggle.svelte';
	import ListRowLink from '$lib/components/list-row-link.svelte';
	import { browseGutterClass, browseItemClass, browseListClass } from '$lib/list-row';
	import type { PageProps } from './$types';
	import { pageTitle } from '$lib/page-title';
	import { pageHeadingClass, pageEmptyTextClass } from '$lib/page-layout';

	// リンクの一覧のファイルを開いた画面 (→ docs/ui.md「リンクの一覧のファイル」)。
	// フォルダの中・アーカイブのアイテム・file コンテンツのどれも、この1つの画面で開く。
	let { data }: PageProps = $props();

	let contentId = $derived(data.contentId);
	let target = $derived(data.target);
	let linksFile = $derived(data.linksFile);

	// フォルダの中なら、ファイルの手前のディレクトリをパンくずに並べる (フォルダの画面と同じ形)。
	let folderCrumbs = $derived(pathCrumbs(target.path ?? '').slice(0, -1));

	function folderHref(path: string): string {
		return withQuery(resolve('/folders/[id]', { id: String(contentId) }), {
			...data.listQuery,
			...(path === '' ? {} : { path })
		});
	}

	let archiveHref = $derived(
		withQuery(resolve('/archives/[id]', { id: String(contentId) }), data.listQuery)
	);

	let downloadHref = $derived(
		target.item !== undefined
			? archiveItemDownloadHref(contentId, target.item)
			: contentDownloadHref(contentId, target.path)
	);

	// カードは覚えている情報ですぐ出し、サーバーに取り直しを頼んだ答えで差し替える (→ docs/ui.md「リンクのカード」)。
	const refreshed = new RefreshedLinkPreviews(() =>
		linksFile.links?.length ? { contentIds: [], linksFile: { contentId, ...target } } : undefined
	);
</script>

<svelte:head><title>{pageTitle(linksFile.title)}</title></svelte:head>

<div class="py-6">
	<div class={browseGutterClass}>
		<BrowseBreadcrumb ancestors={linksFile.ancestors}>
			{#if linksFile.containerTitle != null}
				<span aria-hidden="true">/</span>
				{#if target.item !== undefined}
					<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
					<a href={archiveHref} class={breadcrumbLinkClass}>{linksFile.containerTitle}</a>
				{:else}
					<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
					<a href={folderHref('')} class={breadcrumbLinkClass}>{linksFile.containerTitle}</a>
					{#each folderCrumbs as crumb (crumb.path)}
						<span aria-hidden="true">/</span>
						<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
						<a href={folderHref(crumb.path)} class={breadcrumbLinkClass}>{crumb.name}</a>
					{/each}
				{/if}
			{/if}
			<span aria-hidden="true">/</span>
			<span>{linksFile.name}</span>
		</BrowseBreadcrumb>

		<h1 class={pageHeadingClass}>{linksFile.title}</h1>

		{#if linksFile.links?.length}
			<div class="mb-4 flex justify-end">
				<BrowseLayoutToggle />
			</div>
		{/if}
	</div>

	{#if linksFile.links == null}
		<div class={['flex flex-col items-start gap-2', browseGutterClass]}>
			<p>{m.links_file_error()}</p>
			<!-- API への直リンク (→ $lib/api/urls.ts)。 -->
			<a
				href={downloadHref}
				class="text-primary underline underline-offset-4"
				target="_blank"
				rel="external noopener noreferrer"
			>
				{m.contents_opens_in_new_tab()}
			</a>
		</div>
	{:else if linksFile.links.length === 0}
		<p class={pageEmptyTextClass}>{m.links_file_empty()}</p>
	{:else}
		<ul class={browseListClass()}>
			{#each linksFile.links as link, index (`${index}:${link.url}`)}
				{@const preview = refreshed.get(link.url) ?? link.preview}
				<li class={browseItemClass()}>
					<ListRowLink href={link.url} description={link.note} {preview} />
				</li>
			{/each}
		</ul>
	{/if}
</div>
