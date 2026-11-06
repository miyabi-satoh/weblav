<script lang="ts">
	import FilterHighlight from '$lib/components/filter-highlight.svelte';
	import { listFilter } from '$lib/list-filter.svelte';
	import ListFilterEmpty from '$lib/components/list-filter-empty.svelte';
	import { resolve } from '$app/paths';
	import FileIcon from '@lucide/svelte/icons/file';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import { actionMenuTriggerClass } from '$lib/table-columns';
	import { contentDownloadHref, contentRemoteHref, contentThumbnailHref } from '$lib/api/urls';
	import { contentTypeIcon } from '$lib/content-types';
	import {
		isAudioFileName,
		isLinksFileName,
		remoteFileKind,
		remoteFileName,
		viewerFileKind
	} from '$lib/file-kind';
	import { linksFileHref, LinksFileIcon } from '$lib/links-file';
	import { RefreshedLinkPreviews } from '$lib/link-previews.svelte';
	import type { ViewerFile } from '$lib/file-viewer.svelte';
	import type { Track } from '$lib/now-playing.svelte';
	import type { ViewerImage } from '$lib/image-viewer';
	import { linkDetailOf, opensLinkDetail } from '$lib/link-detail.svelte';
	import { viewerItems } from '$lib/viewer-items';
	import { browseLayout } from '$lib/browse-layout.svelte';
	import { videoEmbedUrl } from '$lib/video-embed';
	import {
		browseItemClass,
		browseListClass,
		browseRowClass,
		browseRowPressClass,
		browseRowTextClass,
		browseRowTitleClass,
		rowMenuSlotClass
	} from '$lib/list-row';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import ListRowIcon from '$lib/components/list-row-icon.svelte';
	import ListRowPlayButton from '$lib/components/list-row-play-button.svelte';
	import ListRowImageLink from '$lib/components/list-row-image-link.svelte';
	import ListRowFileLink from '$lib/components/list-row-file-link.svelte';
	import ListRowDownloadLink from '$lib/components/list-row-download-link.svelte';
	import ListRowLink from '$lib/components/list-row-link.svelte';
	import ListRowSubtitle from '$lib/components/list-row-subtitle.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import type { RowLocation } from '$lib/row-location';
	import type { components } from '$lib/api/schema';

	type ContentEntry = components['schemas']['ContentResponse'];
	type LinkEntry = Extract<ContentEntry, { type: 'link' }>;

	// トップページ(`/`)・`/groups/[id]`・検索の結果で使う一覧。片方だけの改修で
	// もう片方が古びるのを防ぐため、コンテンツ種別ごとの表示分岐をここに集約する。
	let {
		entries: allEntries,
		subtitle = (content) => content.description,
		location,
		actions = false
	}: {
		entries: ContentEntry[];
		/** 行の2段目。省くと説明を出す。検索では、説明で当たった行だけに説明を出す。 */
		subtitle?: (content: ContentEntry) => string | null | undefined;
		/** 検索の結果の、どこにあるか。2段目の、説明の前に出す。 */
		location?: (content: ContentEntry) => RowLocation | undefined;
		/** 書き換えられる行 (`canEdit`) に編集ページへのペンを出す。ホームとグループの一覧で使う。 */
		actions?: boolean;
	} = $props();

	// ページ内の絞り込みが置かれていれば、タイトルで行を絞る (→ docs/ui.md「一覧の絞り込み」)。
	const filter = listFilter();
	let entries = $derived(filter ? filter.apply(allEntries, (item) => item.title) : allEntries);

	/** URL のファイルを指す link コンテンツなら、その種類。サーバーの中継で、ファイルと同じ行・ビューアーで開く (→ docs/ui.md「URL のファイル」)。 */
	function remoteKind(content: ContentEntry) {
		return content.type === 'link' ? remoteFileKind(content.url) : undefined;
	}

	function isAudioContent(content: ContentEntry): boolean {
		return (
			(content.type === 'file' && !content.missing && isAudioFileName(content.fileName)) ||
			remoteKind(content) === 'audio'
		);
	}

	function toTrack(content: ContentEntry): Track {
		if (content.type === 'link') {
			return { src: contentRemoteHref(content.id), title: content.title, originalUrl: content.url };
		}
		return { src: contentDownloadHref(content.id), title: content.title };
	}

	let audioQueue = $derived(entries.filter(isAudioContent).map(toTrack));

	/** 画像の file コンテンツなら、ビューアーに渡す形。大きさが分からない (読めない) 画像は普通のファイルの行にする。 */
	function toViewerImage(content: ContentEntry): ViewerImage | undefined {
		if (content.type !== 'file' || content.missing || !content.image) return undefined;
		return {
			src: contentDownloadHref(content.id),
			thumbnailSrc: contentThumbnailHref(content.id),
			width: content.image.width,
			height: content.image.height,
			title: content.title
		};
	}

	/** 画像でない file コンテンツの行に縮小画像を出すなら、その URL (→ docs/ui.md「画像のプレビュー」)。 */
	function rowThumbnail(content: ContentEntry): { src: string; original: string } | undefined {
		if (content.type !== 'file' || !content.thumbnail) return undefined;
		return { src: contentThumbnailHref(content.id), original: contentDownloadHref(content.id) };
	}

	/** PDF・動画・テキストなど、ビューアーで開く file コンテンツなら、ビューアーに渡す形 (→ docs/ui.md「PDF・動画・テキストのビューアー」)。 */
	function toViewerFile(content: ContentEntry): ViewerFile | undefined {
		const embed = toViewerEmbed(content);
		if (embed) return embed;
		const remote = remoteKind(content);
		if (content.type === 'link' && remote && remote !== 'audio') {
			return {
				src: contentRemoteHref(content.id),
				title: content.title,
				fileName: remoteFileName(content.url) ?? '',
				kind: remote,
				originalUrl: content.url
			};
		}
		// 先が無いショートカットはビューアーに渡さず、開いて「見つかりません」を出す (→ docs/folders.md「ショートカット」)。
		if (content.type !== 'file' || content.missing) return undefined;
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

	let viewerList = $derived(viewerItems(entries, toViewerImage, toViewerFile));

	/**
	 * 動画サイトの動画を指す link コンテンツなら、ビューアーに渡す形 (→ docs/ui.md「動画サイトの埋め込み」)。
	 * 行はリンクのカードのまま置き、サムネイルと題名を出す。
	 */
	function toViewerEmbed(content: ContentEntry): ViewerFile | undefined {
		if (!isLinkCard(content)) return undefined;
		const src = videoEmbedUrl(content.url);
		if (!src) return undefined;
		return { src, title: content.title, fileName: '', kind: 'embed', originalUrl: content.url };
	}

	/** リンクのカードで出す link コンテンツ。URL のファイルはファイルの行で出す。 */
	function isLinkCard(content: ContentEntry): content is LinkEntry {
		return content.type === 'link' && !remoteKind(content);
	}

	// リンクのカードは覚えている情報ですぐ出し、サーバーに取り直しを頼んだ答えで差し替える
	// (→ docs/ui.md「リンクのカード」)。取り直せなくても、覚えている情報のまま出しておけば足りる。
	const refreshed = new RefreshedLinkPreviews(() => {
		const contentIds = entries.filter(isLinkCard).map((entry) => entry.id);
		return contentIds.length === 0 ? undefined : { contentIds };
	});

	// 詳しい表示で前後へ移る、詳しい表示を出すリンクの並び (→ docs/ui.md「リンクのカード」)。
	let linkDetailEntries = $derived(
		entries
			.filter(isLinkCard)
			.filter((content) => opensLinkDetail(content.url, !!toViewerFile(content)))
	);
	let linkDetails = $derived(
		linkDetailEntries.map((content) =>
			linkDetailOf({
				href: content.url,
				title: content.title,
				description: subtitle(content),
				preview: refreshed.get(content.url) ?? content.preview
			})
		)
	);
	let linkDetailIndex = $derived(
		new Map(linkDetailEntries.map((content, index) => [content.id, index]))
	);

	const errorDialog = new ErrorDialogState();
</script>

{#snippet rowMenu(content: ContentEntry)}
	{#if actions && content.canEdit}
		<!-- 操作は編集だけなので、⋮ のメニューにせず、見出しと同じペンを ⋮ と同じ所・大きさで置く (→ docs/ui.md「UI 全般」)。 -->
		<span class={rowMenuSlotClass()}>
			<a
				href={resolve('/contents/[id]', { id: String(content.id) })}
				class={actionMenuTriggerClass}
				aria-label={m.browse_heading_edit_label({ title: content.title })}
			>
				<PencilIcon class="size-4" />
			</a>
		</span>
	{/if}
{/snippet}

{#snippet rowTitle(content: ContentEntry)}
	<span class={browseRowTitleClass()}><FilterHighlight text={content.title} /></span>
{/snippet}

<!-- 作成者にしか届かない行の印 (→ docs/ui.md「UI 全般」)。タイルは幅が狭いので2段目の頭に置く。 -->
{#snippet rowSubtitle(content: ContentEntry)}
	<ListRowSubtitle
		prefix={content.private && browseLayout.tile ? m.contents_visibility_private() : undefined}
		location={location?.(content)}
		text={subtitle(content)}
	/>
{/snippet}

<!-- 行の右端。「作成者のみ」の印の右に ⋮ を置く。 -->
{#snippet rowTrailing(content: ContentEntry)}
	{#if content.private && !browseLayout.tile}
		<span class="shrink-0 text-sm text-muted-foreground">{m.contents_visibility_private()}</span>
	{/if}
	{@render rowMenu(content)}
{/snippet}

{#if filter?.active && entries.length === 0}
	<ListFilterEmpty />
{:else}
	<ul class={browseListClass()}>
		{#each entries as content (content.id)}
			{@const image = toViewerImage(content)}
			{@const viewerFile = toViewerFile(content)}
			<li class={browseItemClass()}>
				{#if isLinkCard(content)}
					<ListRowLink
						href={content.url}
						title={content.title}
						description={subtitle(content)}
						location={location?.(content)}
						preview={refreshed.get(content.url) ?? content.preview}
						private={content.private}
						viewer={viewerFile && { file: viewerFile, items: viewerList }}
						details={{ items: linkDetails, index: linkDetailIndex.get(content.id) ?? -1 }}
					>
						{#snippet trailing()}{@render rowMenu(content)}{/snippet}
					</ListRowLink>
				{:else if content.type === 'file' && content.missing}
					<!-- 先が無いショートカット。行は残し、押すとその場で「見つかりません」と伝える (→ docs/folders.md「ショートカット」)。 -->
					<div class={browseRowClass()}>
						<ListRowIcon icon={FileIcon} />
						<span class={browseRowTextClass()}>
							<button
								type="button"
								class={[browseRowPressClass(), 'block w-full text-left']}
								onclick={() => errorDialog.show(m.contents_shortcut_missing())}
							>
								{@render rowTitle(content)}
							</button>
							{@render rowSubtitle(content)}
						</span>
						{@render rowTrailing(content)}
					</div>
				{:else if content.type === 'folder' || content.type === 'group' || (content.type === 'file' && isLinksFileName(content.fileName))}
					<div class={browseRowClass()}>
						{#if content.type === 'file'}
							<ListRowIcon icon={LinksFileIcon} />
						{:else}
							<ListRowIcon icon={contentTypeIcon(content.type)} />
						{/if}
						<span class={browseRowTextClass()}>
							<!-- 一覧の表示を使うフォルダーは、そちらを入口にする (→ docs/folders.md「2つの表示」)。 -->
							{#if content.type === 'folder' && content.listView}
								<a
									href={resolve('/archives/[id]', { id: String(content.id) })}
									class={browseRowPressClass()}>{@render rowTitle(content)}</a
								>
							{:else if content.type === 'folder'}
								<a
									href={resolve('/folders/[id]', { id: String(content.id) })}
									class={browseRowPressClass()}>{@render rowTitle(content)}</a
								>
							{:else if content.type === 'group'}
								<a
									href={resolve('/groups/[id]', { id: String(content.id) })}
									class={browseRowPressClass()}>{@render rowTitle(content)}</a
								>
							{:else}
								<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
								<a href={linksFileHref(content.id)} class={browseRowPressClass()}
									>{@render rowTitle(content)}</a
								>
							{/if}
							{@render rowSubtitle(content)}
						</span>
						{@render rowTrailing(content)}
					</div>
				{:else if isAudioContent(content)}
					<ListRowPlayButton track={toTrack(content)} queue={audioQueue}>
						{@render rowTitle(content)}
						{#snippet subtitle()}{@render rowSubtitle(content)}{/snippet}
						{#snippet trailing()}{@render rowTrailing(content)}{/snippet}
					</ListRowPlayButton>
				{:else if image}
					<ListRowImageLink {image} items={viewerList}>
						{@render rowTitle(content)}
						{#snippet subtitle()}{@render rowSubtitle(content)}{/snippet}
						{#snippet trailing()}{@render rowTrailing(content)}{/snippet}
					</ListRowImageLink>
				{:else if viewerFile}
					<ListRowFileLink file={viewerFile} items={viewerList}>
						{@render rowTitle(content)}
						{#snippet subtitle()}{@render rowSubtitle(content)}{/snippet}
						{#snippet trailing()}{@render rowTrailing(content)}{/snippet}
					</ListRowFileLink>
				{:else if content.type === 'file'}
					<ListRowDownloadLink
						href={contentDownloadHref(content.id)}
						thumbnail={rowThumbnail(content)}
					>
						{@render rowTitle(content)}
						{#snippet subtitle()}{@render rowSubtitle(content)}{/snippet}
						{#snippet trailing()}{@render rowTrailing(content)}{/snippet}
					</ListRowDownloadLink>
				{/if}
			</li>
		{/each}
	</ul>
{/if}

{#if actions}

{/if}
<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
