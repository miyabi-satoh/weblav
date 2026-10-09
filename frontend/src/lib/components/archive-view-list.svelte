<script lang="ts" module>
	import type { components } from '$lib/api/schema';

	type ArchiveViewItem = components['schemas']['ArchiveViewItem'];

	/** 1行分。検索では、ほかのアーカイブの行も並ぶので、行ごとにアーカイブを持つ。 */
	export type ArchiveViewRow = {
		archiveId: number;
		item: ArchiveViewItem;
		/** 2段目。配列なら「·」で区切って並べる。省くと、アイテムの軸の値 (`item.subtitle`) を出す。 */
		subtitle?: string | string[] | null;
	};
</script>

<script lang="ts">
	import FilterHighlight from '$lib/components/filter-highlight.svelte';
	import { listFilter } from '$lib/list-filter.svelte';
	import ListFilterEmpty from '$lib/components/list-filter-empty.svelte';
	import { archiveItemDownloadHref, archiveItemThumbnailHref } from '$lib/api/urls';
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
	import SeparatedText from '$lib/components/separated-text.svelte';
	import ListRowIcon from '$lib/components/list-row-icon.svelte';
	import ListRowPlayButton from '$lib/components/list-row-play-button.svelte';
	import ListRowImageLink from '$lib/components/list-row-image-link.svelte';
	import ListRowFileLink from '$lib/components/list-row-file-link.svelte';
	import FileIcon from '@lucide/svelte/icons/file';

	// アーカイブの一覧 (`/archives/[id]`) と検索の結果で使う、アーカイブのファイルの行。
	// 押したときの動きを両方で揃えるため、種類ごとの描き分けをここに集める。
	let {
		rows: allRows,
		linksHref
	}: { rows: ArchiveViewRow[]; linksHref: (row: ArchiveViewRow) => string } = $props();

	// ページ内の絞り込みが置かれていれば、タイトルで行を絞る (→ docs/ui.md「一覧の絞り込み」)。
	const filter = listFilter();
	let rows = $derived(filter ? filter.apply(allRows, (item) => item.item.title) : allRows);

	function downloadHref(row: ArchiveViewRow): string {
		return archiveItemDownloadHref(row.archiveId, row.item.id);
	}

	function toTrack(row: ArchiveViewRow): Track {
		return { src: downloadHref(row), title: row.item.title };
	}

	// 続けて鳴らすのは、この一覧の中の音声。
	let audioQueue = $derived(rows.filter((row) => isAudioFileName(row.item.fileName)).map(toTrack));

	/** 画像の行なら、ビューアーに渡す形。大きさが分からない (読めない) 画像は普通のファイルの行にする。 */
	function toViewerImage(row: ArchiveViewRow): ViewerImage | undefined {
		if (!row.item.image) return undefined;
		return {
			src: downloadHref(row),
			thumbnailSrc: archiveItemThumbnailHref(row.archiveId, row.item.id),
			width: row.item.image.width,
			height: row.item.image.height,
			title: row.item.title
		};
	}

	/** 画像でない行に縮小画像を出すなら、その URL (→ docs/ui.md「画像のプレビュー」)。 */
	function rowThumbnail(row: ArchiveViewRow): { src: string; original: string } | undefined {
		if (!row.item.thumbnail) return undefined;
		return {
			src: archiveItemThumbnailHref(row.archiveId, row.item.id),
			original: downloadHref(row)
		};
	}

	/** PDF・動画・テキストなど、ビューアーで開く行なら、ビューアーに渡す形 (→ docs/ui.md「PDF・動画・テキストのビューアー」)。 */
	function toViewerFile(row: ArchiveViewRow): ViewerFile | undefined {
		const kind = viewerFileKind(row.item.fileName, row.item.isText);
		if (!kind) return undefined;
		return {
			src: downloadHref(row),
			title: row.item.title,
			fileName: row.item.fileName,
			kind,
			thumbnail: rowThumbnail(row)
		};
	}

	let viewerList = $derived(viewerItems(rows, toViewerImage, toViewerFile));
</script>

<!-- 2段目は、アーカイブの一覧では同じタイトルが並んだときに見分けるための軸の値 (→ docs/ui.md「アーカイブの一覧画面」)。 -->
{#snippet itemText(row: ArchiveViewRow)}
	{@const subtitle = row.subtitle === undefined ? row.item.subtitle : row.subtitle}
	<span class={browseRowTextClass()}>
		<span class={browseRowTitleClass()}><FilterHighlight text={row.item.title} /></span>
		{#if subtitle}
			<span class={browseRowSubtitleClass()}><SeparatedText text={subtitle} /></span>
		{/if}
	</span>
{/snippet}

{#if filter?.active && rows.length === 0}
	<ListFilterEmpty />
{:else}
	<ul class={browseListClass()}>
		{#each rows as row (`${row.archiveId}-${row.item.id}`)}
			{@const image = toViewerImage(row)}
			{@const viewerFile = toViewerFile(row)}
			<li class={browseItemClass()}>
				{#if isLinksFileName(row.item.fileName)}
					<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
					<a href={linksHref(row)} class={browseRowClass()}>
						<ListRowIcon icon={LinksFileIcon} />
						{@render itemText(row)}
						<ListRowGlyph />
					</a>
				{:else if isAudioFileName(row.item.fileName)}
					<ListRowPlayButton track={toTrack(row)} queue={audioQueue}>
						{@render itemText(row)}
					</ListRowPlayButton>
				{:else if image}
					<ListRowImageLink {image} items={viewerList}>
						{@render itemText(row)}
					</ListRowImageLink>
				{:else if viewerFile}
					<ListRowFileLink file={viewerFile} items={viewerList}>
						{@render itemText(row)}
					</ListRowFileLink>
				{:else}
					<!-- API への直リンク (→ $lib/api/urls.ts)。 -->
					<a
						href={downloadHref(row)}
						class={browseRowClass()}
						target="_blank"
						rel="external noopener noreferrer"
					>
						<ListRowIcon icon={FileIcon} thumbnail={rowThumbnail(row)} />
						{@render itemText(row)}
						<ListRowGlyph newTab />
					</a>
				{/if}
			</li>
		{/each}
	</ul>
{/if}
