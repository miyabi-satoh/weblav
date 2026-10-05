<script lang="ts">
	import { resolve } from '$app/paths';
	import { contentDownloadHref, contentThumbnailHref } from '$lib/api/urls';
	import { contentTypeIcon } from '$lib/content-types';
	import { isAudioFileName, isLinksFileName, viewerFileKind } from '$lib/file-kind';
	import { linksFileHref, LinksFileIcon } from '$lib/links-file';
	import { RefreshedLinkPreviews } from '$lib/link-previews.svelte';
	import type { ViewerFile } from '$lib/file-viewer.svelte';
	import type { Track } from '$lib/now-playing.svelte';
	import type { ViewerImage } from '$lib/image-viewer';
	import { browseLayout } from '$lib/browse-layout.svelte';
	import {
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
	import ListRowLink from '$lib/components/list-row-link.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import type { components } from '$lib/api/schema';

	type ContentEntry = components['schemas']['ContentResponse'];

	// トップページ(`/`)と`/groups/[id]`の両方で使う一覧。片方だけの改修で
	// もう片方が古びるのを防ぐため、コンテンツ種別ごとの表示分岐をここに集約する。
	let { entries }: { entries: ContentEntry[] } = $props();

	function isAudioContent(content: ContentEntry): boolean {
		return content.type === 'file' && isAudioFileName(content.fileName);
	}

	function toTrack(content: ContentEntry): Track {
		return { src: contentDownloadHref(content.id), title: content.title };
	}

	let audioQueue = $derived(entries.filter(isAudioContent).map(toTrack));

	/** 画像の file コンテンツなら、ビューアに渡す形。大きさが分からない (読めない) 画像は普通のファイルの行にする。 */
	function toViewerImage(content: ContentEntry): ViewerImage | undefined {
		if (content.type !== 'file' || !content.image) return undefined;
		return {
			src: contentDownloadHref(content.id),
			thumbnailSrc: contentThumbnailHref(content.id),
			width: content.image.width,
			height: content.image.height,
			title: content.title
		};
	}

	let viewerImages = $derived(entries.map(toViewerImage).filter((image) => image !== undefined));

	/** 画像でない file コンテンツの行に縮小画像を出すなら、その URL (→ docs/ui.md「画像のプレビュー」)。 */
	function rowThumbnail(content: ContentEntry): { src: string; original: string } | undefined {
		if (content.type !== 'file' || !content.thumbnail) return undefined;
		return { src: contentThumbnailHref(content.id), original: contentDownloadHref(content.id) };
	}

	/** PDF・動画・テキストなど、ビューアで開く file コンテンツなら、ビューアに渡す形 (→ docs/ui.md「PDF・動画・テキストのビューア」)。 */
	function toViewerFile(content: ContentEntry): ViewerFile | undefined {
		if (content.type !== 'file') return undefined;
		const kind = viewerFileKind(content.fileName, content.isText);
		if (!kind) return undefined;
		return {
			src: contentDownloadHref(content.id),
			title: content.title,
			fileName: content.fileName,
			kind,
			thumbnail: rowThumbnail(content)
		};
	}

	let viewerFiles = $derived(entries.map(toViewerFile).filter((file) => file !== undefined));

	// リンクのカードは覚えている情報ですぐ出し、サーバーに取り直しを頼んだ答えで差し替える
	// (→ docs/ui.md「リンクのカード」)。取り直せなくても、覚えている情報のまま出しておけば足りる。
	const refreshed = new RefreshedLinkPreviews(() => {
		const contentIds = entries.filter((entry) => entry.type === 'link').map((entry) => entry.id);
		return contentIds.length === 0 ? undefined : { contentIds };
	});
</script>

{#snippet rowText(content: ContentEntry)}
	<!-- 作成者にしか届かない行の印 (→ docs/ui.md「UI 全般」)。タイルは幅が狭いので2段目の頭に置く。 -->
	{@const privateInText = content.private && browseLayout.tile}
	<span class={browseRowTextClass()}>
		<span class={browseRowTitleClass()}>{content.title}</span>
		{#if privateInText || content.description}
			<span class={browseRowSubtitleClass()}>
				{#if privateInText}
					{m.contents_visibility_private()}
					{#if content.description}<span aria-hidden="true">·</span>{/if}
				{/if}
				{content.description}
			</span>
		{/if}
	</span>
	{#if content.private && !browseLayout.tile}
		<span class="shrink-0 text-sm text-muted-foreground">{m.contents_visibility_private()}</span>
	{/if}
{/snippet}

{#snippet rowBody(content: ContentEntry, opensNewTab: boolean)}
	<ListRowIcon
		icon={contentTypeIcon(content.type)}
		accent={content.type === 'archive'}
		thumbnail={rowThumbnail(content)}
	/>
	{@render rowText(content)}
	<ListRowGlyph newTab={opensNewTab} />
{/snippet}

<ul class={browseListClass()}>
	{#each entries as content (content.id)}
		{@const image = toViewerImage(content)}
		{@const viewerFile = toViewerFile(content)}
		<li class={browseItemClass()}>
			{#if content.type === 'link'}
				<ListRowLink
					href={content.url}
					title={content.title}
					description={content.description}
					preview={refreshed.get(content.url) ?? content.preview}
					private={content.private}
				/>
			{:else if content.type === 'folder'}
				<a href={resolve('/folders/[id]', { id: String(content.id) })} class={browseRowClass()}>
					{@render rowBody(content, false)}
				</a>
			{:else if content.type === 'archive'}
				<a href={resolve('/archives/[id]', { id: String(content.id) })} class={browseRowClass()}>
					{@render rowBody(content, false)}
				</a>
			{:else if content.type === 'group'}
				<a href={resolve('/groups/[id]', { id: String(content.id) })} class={browseRowClass()}>
					{@render rowBody(content, false)}
				</a>
			{:else if content.type === 'file' && isLinksFileName(content.fileName)}
				<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
				<a href={linksFileHref(content.id)} class={browseRowClass()}>
					<ListRowIcon icon={LinksFileIcon} />
					{@render rowText(content)}
					<ListRowGlyph />
				</a>
			{:else if isAudioContent(content)}
				<ListRowPlayButton track={toTrack(content)} queue={audioQueue}>
					{@render rowText(content)}
				</ListRowPlayButton>
			{:else if image}
				<ListRowImageLink {image} images={viewerImages}>
					{@render rowText(content)}
				</ListRowImageLink>
			{:else if viewerFile}
				<ListRowFileLink file={viewerFile} files={viewerFiles}>
					{@render rowText(content)}
				</ListRowFileLink>
			{:else if content.type === 'file'}
				<!-- API への直リンク (→ $lib/api/urls.ts)。 -->
				<a
					href={contentDownloadHref(content.id)}
					class={browseRowClass()}
					target="_blank"
					rel="external noopener noreferrer"
				>
					{@render rowBody(content, true)}
				</a>
			{/if}
		</li>
	{/each}
</ul>
