<script lang="ts" module>
	import type { components } from '$lib/api/schema';

	type FolderEntry = components['schemas']['FolderEntry'];

	/** 1行分。検索では、ほかのフォルダの行も並ぶので、行ごとにフォルダを持つ。 */
	export type FolderEntryRow = {
		contentId: number;
		/** フォルダの登録パスからの相対パス。エントリ自身の名前まで含む。 */
		path: string;
		entry: FolderEntry;
		/** 2段目。配列なら「·」で区切って並べる。省くと、ファイルの大きさと更新日時を出す。 */
		subtitle?: string | string[];
	};
</script>

<script lang="ts">
	import FilterHighlight from '$lib/components/filter-highlight.svelte';
	import { listFilter } from '$lib/list-filter.svelte';
	import { pageEmptyTextClass } from '$lib/page-layout';
	import * as m from '$lib/paraglide/messages.js';
	import { contentDownloadHref, contentThumbnailHref } from '$lib/api/urls';
	import { formatByteSize, formatDate } from '$lib/format';
	import { isAudioFileName, isLinksFileName, viewerFileKind } from '$lib/file-kind';
	import { LinksFileIcon } from '$lib/links-file';
	import type { ViewerFile } from '$lib/file-viewer.svelte';
	import type { Track } from '$lib/now-playing.svelte';
	import type { ViewerImage } from '$lib/image-viewer';
	import { viewerItems } from '$lib/viewer-items';
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
	import SeparatedText from '$lib/components/separated-text.svelte';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import FileIcon from '@lucide/svelte/icons/file';

	// フォルダの一覧 (`/folders/[id]`) と検索の結果で使う、フォルダの中のファイル・ディレクトリの行。
	// 押したときの動きを両方で揃えるため、種類ごとの描き分けをここに集める。
	let {
		rows: allRows,
		dirHref,
		linksHref
	}: {
		rows: FolderEntryRow[];
		dirHref: (row: FolderEntryRow) => string;
		linksHref: (row: FolderEntryRow) => string;
	} = $props();

	// ページ内の絞り込みが置かれていれば、タイトルで行を絞る (→ docs/ui.md「一覧の絞り込み」)。
	const filter = listFilter();
	let rows = $derived(
		filter?.active ? allRows.filter((item) => filter.match(item.entry.name) !== null) : allRows
	);

	function downloadHref(row: FolderEntryRow): string {
		return contentDownloadHref(row.contentId, row.path);
	}

	/** 画像でない行に縮小画像を出すなら、その URL (→ docs/ui.md「画像のプレビュー」)。 */
	function rowThumbnail(row: FolderEntryRow): { src: string; original: string } | undefined {
		if (!row.entry.thumbnail) return undefined;
		return {
			src: contentThumbnailHref(row.contentId, row.path),
			original: downloadHref(row)
		};
	}

	function toTrack(row: FolderEntryRow): Track {
		return { src: downloadHref(row), title: row.entry.name };
	}

	// 続けて鳴らすのは、この一覧の中の音声。
	let audioQueue = $derived(
		rows.filter((row) => !row.entry.isDir && isAudioFileName(row.entry.name)).map(toTrack)
	);

	/** 画像の行なら、ビューアに渡す形。大きさが分からない (読めない) 画像は普通のファイルの行にする。 */
	function toViewerImage(row: FolderEntryRow): ViewerImage | undefined {
		if (row.entry.isDir || !row.entry.image) return undefined;
		return {
			src: downloadHref(row),
			thumbnailSrc: contentThumbnailHref(row.contentId, row.path),
			width: row.entry.image.width,
			height: row.entry.image.height,
			title: row.entry.name
		};
	}

	/** PDF・動画・テキストなど、ビューアで開く行なら、ビューアに渡す形 (→ docs/ui.md「PDF・動画・テキストのビューア」)。 */
	function toViewerFile(row: FolderEntryRow): ViewerFile | undefined {
		const kind = row.entry.isDir ? undefined : viewerFileKind(row.entry.name, row.entry.isText);
		if (!kind) return undefined;
		return {
			src: downloadHref(row),
			title: row.entry.name,
			fileName: row.entry.name,
			kind,
			thumbnail: rowThumbnail(row)
		};
	}

	let viewerList = $derived(viewerItems(rows, toViewerImage, toViewerFile));
</script>

{#snippet entryText(row: FolderEntryRow)}
	{@const entry = row.entry}
	<span class={browseRowTextClass()}>
		<span class={browseRowTitleClass(true)}><FilterHighlight text={entry.name} /></span>
		{#if row.subtitle !== undefined}
			<span class={browseRowSubtitleClass()}><SeparatedText text={row.subtitle} /></span>
		{:else if !entry.isDir}
			<span class={browseRowSubtitleClass()}>
				{#if entry.size != null}{formatByteSize(entry.size)}{/if}
				{#if entry.size != null && entry.modifiedAt != null}<span aria-hidden="true">·</span>{/if}
				{formatDate(entry.modifiedAt)}
			</span>
		{/if}
	</span>
{/snippet}

{#if filter?.active && rows.length === 0}
	<p class={pageEmptyTextClass}>{m.browse_filter_empty({ query: filter.value.trim() })}</p>
{:else}
	<ul class={browseListClass()}>
		{#each rows as row (`${row.contentId}:${row.path}`)}
			{@const image = toViewerImage(row)}
			{@const viewerFile = toViewerFile(row)}
			<li class={browseItemClass()}>
				{#if row.entry.isDir}
					<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
					<a href={dirHref(row)} class={browseRowClass(true)}>
						<ListRowIcon icon={FolderIcon} compact />
						{@render entryText(row)}
						<ListRowGlyph />
					</a>
				{:else if isLinksFileName(row.entry.name)}
					<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
					<a href={linksHref(row)} class={browseRowClass(true)}>
						<ListRowIcon icon={LinksFileIcon} compact />
						{@render entryText(row)}
						<ListRowGlyph />
					</a>
				{:else if isAudioFileName(row.entry.name)}
					<ListRowPlayButton track={toTrack(row)} queue={audioQueue} compact>
						{@render entryText(row)}
					</ListRowPlayButton>
				{:else if image}
					<ListRowImageLink {image} items={viewerList} compact>
						{@render entryText(row)}
					</ListRowImageLink>
				{:else if viewerFile}
					<ListRowFileLink file={viewerFile} items={viewerList} compact>
						{@render entryText(row)}
					</ListRowFileLink>
				{:else}
					<!-- API への直リンク (→ $lib/api/urls.ts)。 -->
					<a
						href={downloadHref(row)}
						class={browseRowClass(true)}
						target="_blank"
						rel="external noopener noreferrer"
					>
						<ListRowIcon icon={FileIcon} compact thumbnail={rowThumbnail(row)} />
						{@render entryText(row)}
						<ListRowGlyph newTab />
					</a>
				{/if}
			</li>
		{/each}
	</ul>
{/if}
