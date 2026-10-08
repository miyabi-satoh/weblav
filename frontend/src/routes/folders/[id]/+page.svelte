<script lang="ts">
	import { resolve } from '$app/paths';
	import { withQuery } from '$lib/href';
	import * as m from '$lib/paraglide/messages.js';
	import { contentDownloadHref, contentThumbnailHref } from '$lib/api/urls';
	import { formatByteSize, formatDate } from '$lib/format';
	import { isAudioFileName, isLinksFileName, viewerFileKind } from '$lib/file-kind';
	import { linksFileHref, LinksFileIcon } from '$lib/links-file';
	import type { ViewerFile } from '$lib/file-viewer.svelte';
	import type { Track } from '$lib/now-playing.svelte';
	import type { ViewerImage } from '$lib/image-viewer';
	import { viewerItems } from '$lib/viewer-items';
	import { breadcrumbLinkClass, pathCrumbs } from '$lib/breadcrumb';
	import { SORT_OPTIONS, DEFAULT_BROWSE_SORT, type BrowseSort } from '$lib/browse-sort';
	import BrowseBreadcrumb from '$lib/components/browse-breadcrumb.svelte';
	import BrowseLayoutToggle from '$lib/components/browse-layout-toggle.svelte';
	import BrowseSortSelect from '$lib/components/browse-sort.svelte';
	import {
		browseControlsClass,
		browseGutterClass,
		browseItemClass,
		browseListClass,
		browseRowClass,
		browseRowSubtitleClass,
		browseRowTextClass,
		browseRowTitleClass
	} from '$lib/list-row';
	import ListRowGlyph from '$lib/components/list-row-glyph.svelte';
	import ListRowIcon from '$lib/components/list-row-icon.svelte';
	import ListRowPlayButton from '$lib/components/list-row-play-button.svelte';
	import ListRowImageLink from '$lib/components/list-row-image-link.svelte';
	import ListRowFileLink from '$lib/components/list-row-file-link.svelte';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import FileIcon from '@lucide/svelte/icons/file';
	import type { components } from '$lib/api/schema';
	import type { PageProps } from './$types';
	import { pageTitle } from '$lib/page-title';
	import { pageHeadingClass, pageEmptyTextClass } from '$lib/page-layout';

	type FolderEntry = components['schemas']['FolderEntry'];

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

	function linksHref(entry: FolderEntry): string {
		return linksFileHref(
			contentId,
			{ path: childPath(entry.name) },
			data.sort === DEFAULT_BROWSE_SORT ? {} : { sort: data.sort }
		);
	}

	function downloadHref(entry: FolderEntry): string {
		return contentDownloadHref(contentId, childPath(entry.name));
	}

	/** 画像でない行に縮小画像を出すなら、その URL (→ docs/ui.md「画像のプレビュー」)。 */
	function rowThumbnail(entry: FolderEntry): { src: string; original: string } | undefined {
		if (!entry.thumbnail) return undefined;
		return {
			src: contentThumbnailHref(contentId, childPath(entry.name)),
			original: downloadHref(entry)
		};
	}

	function toTrack(entry: FolderEntry): Track {
		return { src: downloadHref(entry), title: entry.name };
	}

	let audioQueue = $derived(
		browse.entries.filter((entry) => !entry.isDir && isAudioFileName(entry.name)).map(toTrack)
	);

	/** 画像の行なら、ビューアに渡す形。大きさが分からない (読めない) 画像は普通のファイルの行にする。 */
	function toViewerImage(entry: FolderEntry): ViewerImage | undefined {
		if (entry.isDir || !entry.image) return undefined;
		return {
			src: downloadHref(entry),
			thumbnailSrc: contentThumbnailHref(contentId, childPath(entry.name)),
			width: entry.image.width,
			height: entry.image.height,
			title: entry.name
		};
	}

	/** PDF・動画・テキストなど、ビューアで開く行なら、ビューアに渡す形 (→ docs/ui.md「PDF・動画・テキストのビューア」)。 */
	function toViewerFile(entry: FolderEntry): ViewerFile | undefined {
		const kind = entry.isDir ? undefined : viewerFileKind(entry.name, entry.isText);
		if (!kind) return undefined;
		return {
			src: downloadHref(entry),
			title: entry.name,
			fileName: entry.name,
			kind,
			thumbnail: rowThumbnail(entry)
		};
	}

	let viewerList = $derived(viewerItems(browse.entries, toViewerImage, toViewerFile));
</script>

<svelte:head><title>{pageTitle(currentName)}</title></svelte:head>

{#snippet entryText(entry: FolderEntry)}
	<span class={browseRowTextClass()}>
		<span class={browseRowTitleClass(true)}>{entry.name}</span>
		{#if !entry.isDir}
			<span class={browseRowSubtitleClass()}>
				{#if entry.size != null}{formatByteSize(entry.size)}{/if}
				{#if entry.size != null && entry.modifiedAt != null}<span aria-hidden="true">·</span>{/if}
				{formatDate(entry.modifiedAt)}
			</span>
		{/if}
	</span>
{/snippet}

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
		<ul class={browseListClass()}>
			{#each browse.entries as entry (entry.name)}
				{@const image = toViewerImage(entry)}
				{@const viewerFile = toViewerFile(entry)}
				<li class={browseItemClass()}>
					{#if entry.isDir}
						<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
						<a href={browseHref(childPath(entry.name))} class={browseRowClass(true)}>
							<ListRowIcon icon={FolderIcon} compact />
							{@render entryText(entry)}
							<ListRowGlyph />
						</a>
					{:else if isLinksFileName(entry.name)}
						<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
						<a href={linksHref(entry)} class={browseRowClass(true)}>
							<ListRowIcon icon={LinksFileIcon} compact />
							{@render entryText(entry)}
							<ListRowGlyph />
						</a>
					{:else if isAudioFileName(entry.name)}
						<ListRowPlayButton track={toTrack(entry)} queue={audioQueue} compact>
							{@render entryText(entry)}
						</ListRowPlayButton>
					{:else if image}
						<ListRowImageLink {image} items={viewerList} compact>
							{@render entryText(entry)}
						</ListRowImageLink>
					{:else if viewerFile}
						<ListRowFileLink file={viewerFile} items={viewerList} compact>
							{@render entryText(entry)}
						</ListRowFileLink>
					{:else}
						<!-- API への直リンク (→ $lib/api/urls.ts)。 -->
						<a
							href={downloadHref(entry)}
							class={browseRowClass(true)}
							target="_blank"
							rel="external noopener noreferrer"
						>
							<ListRowIcon icon={FileIcon} compact thumbnail={rowThumbnail(entry)} />
							{@render entryText(entry)}
							<ListRowGlyph newTab />
						</a>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}
</div>
