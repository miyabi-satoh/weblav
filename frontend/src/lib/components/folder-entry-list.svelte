<script lang="ts" module>
	import type { components } from '$lib/api/schema';
	import type { RowLocation } from '$lib/row-location';

	type FolderEntry = components['schemas']['FolderEntry'];
	type ListedTitle = components['schemas']['ListedTitle'];

	/** 1行分。検索では、ほかのフォルダーの行も並ぶので、行ごとにフォルダーを持つ。 */
	export type FolderEntryRow = {
		contentId: number;
		/** フォルダーの登録パスからの相対パス。エントリ自身の名前まで含む。 */
		path: string;
		entry: FolderEntry;
		/** 検索の結果の、どこにあるか。2段目に出す。省くと、ファイルの大きさと更新日時を出す。 */
		location?: RowLocation;
		/**
		 * 検索で、一覧の表示の索引で当たった行の表示タイトルと軸の値。名前の代わりにタイトルを出し、
		 * 2段目の場所の後ろに軸の値を出す。当たったのがファイル名でなく、表示タイトルと軸の値のため。
		 */
		listed?: ListedTitle | null;
	};

	/** 行に出す名前。 */
	export function folderEntryTitle(row: FolderEntryRow): string {
		return row.listed?.title ?? row.entry.name;
	}
</script>

<script lang="ts">
	import type { Snippet } from 'svelte';
	import FilterHighlight from '$lib/components/filter-highlight.svelte';
	import { listFilter } from '$lib/list-filter.svelte';
	import ListFilterEmpty from '$lib/components/list-filter-empty.svelte';
	import { contentDownloadHref, contentThumbnailHref } from '$lib/api/urls';
	import type { ContentLocation } from '$lib/content-location';
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
		browseRowPressClass,
		browseRowTextClass,
		browseRowTitleClass
	} from '$lib/list-row';
	import ListRowGlyph from '$lib/components/list-row-glyph.svelte';
	import ListRowIcon from '$lib/components/list-row-icon.svelte';
	import ListRowPlayButton from '$lib/components/list-row-play-button.svelte';
	import ListRowImageLink from '$lib/components/list-row-image-link.svelte';
	import ListRowFileLink from '$lib/components/list-row-file-link.svelte';
	import ListRowDownloadLink from '$lib/components/list-row-download-link.svelte';
	import ListRowSubtitle from '$lib/components/list-row-subtitle.svelte';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import * as m from '$lib/paraglide/messages.js';

	// フォルダーの一覧 (`/folders/[id]`) と検索の結果で使う、フォルダーの中のファイル・ディレクトリの行。
	// 押したときの動きを両方で揃えるため、種類ごとの描き分けをここに集める。
	let {
		rows: allRows,
		dirHref,
		linksHref,
		trailing
	}: {
		rows: FolderEntryRow[];
		dirHref: (row: FolderEntryRow) => string;
		linksHref: (row: FolderEntryRow) => string;
		/** 行の右端に置くもの (書き換えられる人の ⋮ メニュー)。 */
		trailing?: Snippet<[FolderEntryRow]>;
	} = $props();

	// ページ内の絞り込みが置かれていれば、タイトルで行を絞る (→ docs/ui.md「一覧の絞り込み」)。
	const filter = listFilter();
	let rows = $derived(filter ? filter.apply(allRows, folderEntryTitle) : allRows);
	// 行の部品の中では `trailing` が同名の口を指すので、外で読んでおく。
	let hasTrailing = $derived(trailing !== undefined);

	function downloadHref(row: FolderEntryRow): string {
		return contentDownloadHref(row.contentId, row.path);
	}

	/** Markdown のプレビューが相対パスを解決する場所。 */
	function location(row: FolderEntryRow): ContentLocation {
		return { path: row.path, src: (path) => contentDownloadHref(row.contentId, path) };
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
		return { src: downloadHref(row), title: folderEntryTitle(row) };
	}

	// 続けて鳴らすのは、この一覧の中の音声。
	let audioQueue = $derived(
		rows.filter((row) => !row.entry.isDir && isAudioFileName(row.entry.name)).map(toTrack)
	);

	/** 画像の行なら、ビューアーに渡す形。大きさが分からない (読めない) 画像は普通のファイルの行にする。 */
	function toViewerImage(row: FolderEntryRow): ViewerImage | undefined {
		if (row.entry.isDir || !row.entry.image) return undefined;
		return {
			src: downloadHref(row),
			thumbnailSrc: contentThumbnailHref(row.contentId, row.path),
			width: row.entry.image.width,
			height: row.entry.image.height,
			title: folderEntryTitle(row),
			location: location(row)
		};
	}

	/** PDF・動画・テキストなど、ビューアーで開く行なら、ビューアーに渡す形 (→ docs/ui.md「PDF・動画・テキストのビューアー」)。 */
	function toViewerFile(row: FolderEntryRow): ViewerFile | undefined {
		const kind = row.entry.isDir ? undefined : viewerFileKind(row.entry.name, row.entry.isText);
		if (!kind) return undefined;
		return {
			src: downloadHref(row),
			title: folderEntryTitle(row),
			fileName: row.entry.name,
			kind,
			thumbnail: rowThumbnail(row),
			location: location(row)
		};
	}

	let viewerList = $derived(viewerItems(rows, toViewerImage, toViewerFile));
</script>

{#snippet entryTitle(row: FolderEntryRow)}
	<span class={browseRowTitleClass(true)}><FilterHighlight text={folderEntryTitle(row)} /></span>
{/snippet}

{#snippet entrySubtitle(row: FolderEntryRow)}
	{@const entry = row.entry}
	<!-- 非公開の行は、書き換えられる人にだけ並ぶ (→ docs/folders.md「公開と非公開」)。 -->
	<ListRowSubtitle
		prefix={entry.public ? undefined : m.marks_private_label()}
		location={row.location}
		text={row.listed
			? row.listed.subtitle
			: row.location || entry.isDir
				? undefined
				: [formatByteSize(entry.size), formatDate(entry.modifiedAt)].filter((part) => part !== '')}
	/>
{/snippet}

{#snippet trailingSlot(row: FolderEntryRow)}{@render trailing?.(row)}{/snippet}

{#if filter?.active && rows.length === 0}
	<ListFilterEmpty />
{:else}
	<ul class={browseListClass()}>
		{#each rows as row (`${row.contentId}:${row.path}`)}
			{@const image = toViewerImage(row)}
			{@const viewerFile = toViewerFile(row)}
			<li class={browseItemClass()}>
				{#if row.entry.isDir}
					<div class={browseRowClass(true)}>
						<ListRowIcon icon={FolderIcon} compact />
						<span class={browseRowTextClass()}>
							<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
							<a href={dirHref(row)} class={browseRowPressClass()}>{@render entryTitle(row)}</a>
							{@render entrySubtitle(row)}
						</span>
						{@render trailing?.(row)}
						<ListRowGlyph />
					</div>
				{:else if isLinksFileName(row.entry.name)}
					<div class={browseRowClass(true)}>
						<ListRowIcon icon={LinksFileIcon} compact />
						<span class={browseRowTextClass()}>
							<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
							<a href={linksHref(row)} class={browseRowPressClass()}>{@render entryTitle(row)}</a>
							{@render entrySubtitle(row)}
						</span>
						{@render trailing?.(row)}
						<ListRowGlyph />
					</div>
				{:else if isAudioFileName(row.entry.name)}
					<ListRowPlayButton track={toTrack(row)} queue={audioQueue} compact>
						{@render entryTitle(row)}
						{#snippet subtitle()}{@render entrySubtitle(row)}{/snippet}
						{#snippet trailing()}{@render trailingSlot(row)}{/snippet}
					</ListRowPlayButton>
				{:else if image}
					<ListRowImageLink {image} items={viewerList} compact>
						{@render entryTitle(row)}
						{#snippet subtitle()}{@render entrySubtitle(row)}{/snippet}
						{#snippet trailing()}{@render trailingSlot(row)}{/snippet}
					</ListRowImageLink>
				{:else if viewerFile}
					<ListRowFileLink file={viewerFile} items={viewerList} compact>
						{@render entryTitle(row)}
						{#snippet subtitle()}{@render entrySubtitle(row)}{/snippet}
						{#snippet trailing()}{@render trailingSlot(row)}{/snippet}
					</ListRowFileLink>
				{:else}
					<ListRowDownloadLink
						href={downloadHref(row)}
						thumbnail={rowThumbnail(row)}
						compact
						tileCornerTaken={hasTrailing}
					>
						{@render entryTitle(row)}
						{#snippet subtitle()}{@render entrySubtitle(row)}{/snippet}
						{#snippet trailing()}{@render trailingSlot(row)}{/snippet}
					</ListRowDownloadLink>
				{/if}
			</li>
		{/each}
	</ul>
{/if}
