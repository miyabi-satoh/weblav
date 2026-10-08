<script lang="ts">
	import { resolve } from '$app/paths';
	import { withQuery } from '$lib/href';
	import * as m from '$lib/paraglide/messages.js';
	import { archiveItemDownloadHref, contentDownloadHref, linksFileRemoteHref } from '$lib/api/urls';
	import { remoteFileKind, remoteFileName } from '$lib/file-kind';
	import type { ViewerFile } from '$lib/file-viewer.svelte';
	import { viewerItems } from '$lib/viewer-items';
	import type { Track } from '$lib/now-playing.svelte';
	import { videoEmbedUrl } from '$lib/video-embed';
	import { breadcrumbLinkClass, pathCrumbs } from '$lib/breadcrumb';
	import { RefreshedLinkPreviews } from '$lib/link-previews.svelte';
	import BrowseBreadcrumb from '$lib/components/browse-breadcrumb.svelte';
	import BrowseLayoutToggle from '$lib/components/browse-layout-toggle.svelte';
	import ListRowFileLink from '$lib/components/list-row-file-link.svelte';
	import ListRowLink from '$lib/components/list-row-link.svelte';
	import ListRowPlayButton from '$lib/components/list-row-play-button.svelte';
	import SeparatedText from '$lib/components/separated-text.svelte';
	import {
		browseGutterClass,
		browseItemClass,
		browseListClass,
		browseRowSubtitleClass,
		browseRowTextClass,
		browseRowTitleClass
	} from '$lib/list-row';
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

	type Link = NonNullable<typeof linksFile.links>[number];

	// URL のファイルと動画サイトの動画は、一覧のコンテンツと同じく、ファイルの行・ビューアで開く
	// (→ docs/ui.md「URL のファイル」「動画サイトの埋め込み」)。
	/** ページのタイトルが取れなければ、ファイルはファイル名、ほかはホスト名 (リンクのカードと同じ)。 */
	function linkTitle(link: Link): string {
		const title = (refreshed.get(link.url) ?? link.preview)?.title;
		if (title) return title;
		if (remoteFileKind(link.url)) return remoteFileName(link.url) ?? link.url;
		return new URL(link.url).host;
	}

	function toTrack(link: Link): Track {
		return {
			src: linksFileRemoteHref(contentId, target, link.url),
			title: linkTitle(link),
			originalUrl: link.url
		};
	}

	function toViewerFile(link: Link): ViewerFile | undefined {
		const embed = videoEmbedUrl(link.url);
		if (embed) {
			return {
				src: embed,
				title: linkTitle(link),
				fileName: '',
				kind: 'embed',
				originalUrl: link.url
			};
		}
		const kind = remoteFileKind(link.url);
		if (!kind || kind === 'audio') return undefined;
		return {
			src: linksFileRemoteHref(contentId, target, link.url),
			title: linkTitle(link),
			fileName: remoteFileName(link.url) ?? '',
			kind,
			originalUrl: link.url
		};
	}

	let links = $derived(linksFile.links ?? []);
	let audioQueue = $derived(
		links.filter((link) => remoteFileKind(link.url) === 'audio').map(toTrack)
	);
	let viewerList = $derived(viewerItems(links, () => undefined, toViewerFile));

	// カードは覚えている情報ですぐ出し、サーバーに取り直しを頼んだ答えで差し替える (→ docs/ui.md「リンクのカード」)。
	const refreshed = new RefreshedLinkPreviews(() =>
		linksFile.links?.length ? { contentIds: [], linksFile: { contentId, ...target } } : undefined
	);
</script>

{#snippet rowText(link: Link)}
	<span class={browseRowTextClass()}>
		<span class={browseRowTitleClass()}>{linkTitle(link)}</span>
		{#if link.note}
			<span class={browseRowSubtitleClass()}><SeparatedText text={link.note} /></span>
		{/if}
	</span>
{/snippet}

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
				{@const kind = remoteFileKind(link.url)}
				{@const viewerFile = toViewerFile(link)}
				<li class={browseItemClass()}>
					{#if kind === 'audio'}
						<ListRowPlayButton track={toTrack(link)} queue={audioQueue}>
							{@render rowText(link)}
						</ListRowPlayButton>
					{:else if kind && viewerFile}
						<ListRowFileLink file={viewerFile} items={viewerList}>
							{@render rowText(link)}
						</ListRowFileLink>
					{:else}
						<ListRowLink
							href={link.url}
							description={link.note}
							{preview}
							viewer={viewerFile && { file: viewerFile, items: viewerList }}
						/>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}
</div>
