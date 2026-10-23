<script lang="ts" module>
	import type { components } from '$lib/api/schema';
	import {
		archiveFileHref,
		archiveItemDownloadHref,
		archiveItemThumbnailHref
	} from '$lib/api/urls';

	type ArchiveViewItem = components['schemas']['ArchiveViewItem'];

	/** 1行分。 */
	export type ArchiveViewRow = {
		archiveId: number;
		item: ArchiveViewItem;
	};

	/** 行が開く・縮小画像を出す URL。管理画面は、未公開も渡す管理用の口を使う。 */
	export type ArchiveItemUrls = {
		download: (archiveId: number, itemId: number) => string;
		thumbnail: (archiveId: number, itemId: number) => string;
		/** 登録先からの相対パスで指したアイテム。Markdown のプレビューの相対パスに使う。 */
		file: (archiveId: number, path: string) => string;
	};

	const BROWSE_URLS: ArchiveItemUrls = {
		download: archiveItemDownloadHref,
		thumbnail: archiveItemThumbnailHref,
		file: archiveFileHref
	};
</script>

<script lang="ts">
	import FilterHighlight from '$lib/components/filter-highlight.svelte';
	import { listFilter } from '$lib/list-filter.svelte';
	import ListFilterEmpty from '$lib/components/list-filter-empty.svelte';
	import type { Snippet } from 'svelte';
	import type { ContentLocation } from '$lib/content-location';
	import { isAudioFileName, isLinksFileName, viewerFileKind } from '$lib/file-kind';
	import { LinksFileIcon } from '$lib/links-file';
	import type { ViewerFile } from '$lib/file-viewer.svelte';
	import type { Track } from '$lib/now-playing.svelte';
	import type { ViewerImage } from '$lib/image-viewer';
	import { viewerItems } from '$lib/viewer-items';
	import { WindowVirtualizer } from 'virtua/svelte';
	import {
		browseItemClass,
		browseRowClass,
		browseRowPressClass,
		browseRowsPerLine,
		browseRowTextClass,
		browseRowTitleClass,
		browseVirtualLineClass,
		browseVirtualLineStyle,
		browseVirtualListClass
	} from '$lib/list-row';
	import ListRowGlyph from '$lib/components/list-row-glyph.svelte';
	import ListRowSubtitle from '$lib/components/list-row-subtitle.svelte';
	import ListRowIcon from '$lib/components/list-row-icon.svelte';
	import ListRowPlayButton from '$lib/components/list-row-play-button.svelte';
	import ListRowImageLink from '$lib/components/list-row-image-link.svelte';
	import ListRowFileLink from '$lib/components/list-row-file-link.svelte';
	import ListRowDownloadLink from '$lib/components/list-row-download-link.svelte';

	// 一覧の表示 (`/archives/[id]`)・管理画面のアイテムのタブで使う、アーカイブのファイルの行。
	// 押したときの動きと見え方を揃えるため、種類ごとの描き分けをここに集める。
	let {
		rows: allRows,
		linksHref,
		urls = BROWSE_URLS,
		matches,
		leading,
		trailing,
		subtitle: subtitleOverride,
		isHidden,
		hiddenRow
	}: {
		rows: ArchiveViewRow[];
		linksHref: (row: ArchiveViewRow) => string;
		urls?: ArchiveItemUrls;
		/** ページ内の絞り込みで残す行か。無ければ表示タイトルで照らす。 */
		matches?: (row: ArchiveViewRow) => boolean;
		/** 行の前に置くもの (管理画面の選択のチェック)。 */
		leading?: Snippet<[ArchiveViewRow]>;
		/** 行の後ろに置くもの (管理画面の公開の切り替え)。 */
		trailing?: Snippet<[ArchiveViewRow]>;
		/** 2段目を差し替える (管理画面はパスと状態も出す)。 */
		subtitle?: Snippet<[ArchiveViewRow]>;
		/** この画面で非公開にした行か。真なら、行の代わりに `hiddenRow` を出す。 */
		isHidden?: (row: ArchiveViewRow) => boolean;
		hiddenRow?: Snippet<[ArchiveViewRow]>;
	} = $props();

	// 行の部品の中では `trailing` が同名の口を指すので、外で読んでおく。
	let hasTrailing = $derived(trailing !== undefined);

	// ページ内の絞り込みが置かれていれば、行を絞る (→ docs/ui.md「一覧の絞り込み」)。
	const filter = listFilter();
	let rows = $derived(
		matches && filter?.active
			? allRows.filter(matches)
			: filter
				? filter.apply(allRows, (row) => row.item.title)
				: allRows
	);

	function rowKey(row: ArchiveViewRow): string {
		return `${row.archiveId}-${row.item.id}`;
	}

	// 見えている所だけを描くので、行を1段ずつに分けて渡す。タイルは1段に何枚も並ぶ。
	let rowsPerLine = $derived(browseRowsPerLine());
	let lines = $derived.by(() => {
		const result: ArchiveViewRow[][] = [];
		for (let i = 0; i < rows.length; i += rowsPerLine) result.push(rows.slice(i, i + rowsPerLine));
		return result;
	});

	// 非公開にした行は、続けて鳴らす音声やビューアーの前後にも入れない。
	let shownRows = $derived(isHidden ? rows.filter((row) => !isHidden(row)) : rows);

	function downloadHref(row: ArchiveViewRow): string {
		return urls.download(row.archiveId, row.item.id);
	}

	/** Markdown のプレビューが相対パスを解決する場所。相対パスの先は、アイテムの番号でなくパスで引く。 */
	function location(row: ArchiveViewRow): ContentLocation {
		return { path: row.item.relPath, src: (path) => urls.file(row.archiveId, path) };
	}

	function toTrack(row: ArchiveViewRow): Track {
		return { src: downloadHref(row), title: row.item.title };
	}

	// 続けて鳴らすのは、この一覧の中の音声。
	let audioQueue = $derived(
		shownRows.filter((row) => isAudioFileName(row.item.fileName)).map(toTrack)
	);

	/** 画像の行なら、ビューアーに渡す形。大きさが分からない (読めない) 画像は普通のファイルの行にする。 */
	function toViewerImage(row: ArchiveViewRow): ViewerImage | undefined {
		if (!row.item.image) return undefined;
		return {
			src: downloadHref(row),
			thumbnailSrc: urls.thumbnail(row.archiveId, row.item.id),
			width: row.item.image.width,
			height: row.item.image.height,
			title: row.item.title,
			location: location(row)
		};
	}

	/** 画像でない行に縮小画像を出すなら、その URL (→ docs/ui.md「画像のプレビュー」)。 */
	function rowThumbnail(row: ArchiveViewRow): { src: string; original: string } | undefined {
		if (!row.item.thumbnail) return undefined;
		return {
			src: urls.thumbnail(row.archiveId, row.item.id),
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
			thumbnail: rowThumbnail(row),
			location: location(row)
		};
	}

	let viewerList = $derived(viewerItems(shownRows, toViewerImage, toViewerFile));
</script>

{#snippet itemTitle(row: ArchiveViewRow)}
	<span class={browseRowTitleClass()}><FilterHighlight text={row.item.title} /></span>
{/snippet}

<!-- 2段目は、アーカイブの一覧では同じタイトルが並んだときに見分けるための軸の値 (→ docs/ui.md「アーカイブの一覧画面」)。 -->
{#snippet itemSubtitle(row: ArchiveViewRow)}
	{#if subtitleOverride}
		{@render subtitleOverride(row)}
	{:else}
		<ListRowSubtitle text={row.item.subtitle} />
	{/if}
{/snippet}

<!-- 行の部品の同名の口へ渡すため、別名で包む。 -->
{#snippet leadingSlot(row: ArchiveViewRow)}{@render leading?.(row)}{/snippet}
{#snippet trailingSlot(row: ArchiveViewRow)}{@render trailing?.(row)}{/snippet}

<!-- 1つの行の中身。種類ごとに描き分ける。 -->
{#snippet itemRow(row: ArchiveViewRow)}
	{@const image = toViewerImage(row)}
	{@const viewerFile = toViewerFile(row)}
	{#if isHidden?.(row) && hiddenRow}
		{@render hiddenRow(row)}
	{:else if isLinksFileName(row.item.fileName)}
		<div class={browseRowClass()}>
			{@render leading?.(row)}
			<ListRowIcon icon={LinksFileIcon} />
			<span class={browseRowTextClass()}>
				<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
				<a href={linksHref(row)} class={browseRowPressClass()}>{@render itemTitle(row)}</a>
				{@render itemSubtitle(row)}
			</span>
			{@render trailing?.(row)}
			<ListRowGlyph />
		</div>
	{:else if isAudioFileName(row.item.fileName)}
		<ListRowPlayButton track={toTrack(row)} queue={audioQueue}>
			{@render itemTitle(row)}
			{#snippet subtitle()}{@render itemSubtitle(row)}{/snippet}
			{#snippet leading()}{@render leadingSlot(row)}{/snippet}
			{#snippet trailing()}{@render trailingSlot(row)}{/snippet}
		</ListRowPlayButton>
	{:else if image}
		<ListRowImageLink {image} items={viewerList}>
			{@render itemTitle(row)}
			{#snippet subtitle()}{@render itemSubtitle(row)}{/snippet}
			{#snippet leading()}{@render leadingSlot(row)}{/snippet}
			{#snippet trailing()}{@render trailingSlot(row)}{/snippet}
		</ListRowImageLink>
	{:else if viewerFile}
		<ListRowFileLink file={viewerFile} items={viewerList}>
			{@render itemTitle(row)}
			{#snippet subtitle()}{@render itemSubtitle(row)}{/snippet}
			{#snippet leading()}{@render leadingSlot(row)}{/snippet}
			{#snippet trailing()}{@render trailingSlot(row)}{/snippet}
		</ListRowFileLink>
	{:else}
		<ListRowDownloadLink
			href={downloadHref(row)}
			thumbnail={rowThumbnail(row)}
			tileCornerTaken={hasTrailing}
		>
			{@render itemTitle(row)}
			{#snippet subtitle()}{@render itemSubtitle(row)}{/snippet}
			{#snippet leading()}{@render leadingSlot(row)}{/snippet}
			{#snippet trailing()}{@render trailingSlot(row)}{/snippet}
		</ListRowDownloadLink>
	{/if}
{/snippet}

{#if filter?.active && rows.length === 0}
	<ListFilterEmpty />
{:else}
	<!-- 見えている所だけを描く。アーカイブは数万件になり、全部の行を描くとブラウザーのメモリが尽きて画面が落ちるため。 -->
	<div role="list" class={browseVirtualListClass()}>
		<!-- 1段の枚数が変わると段の組み方がすべて変わるので、測った高さを捨てて作り直す。 -->
		{#key rowsPerLine}
			<WindowVirtualizer data={lines} getKey={(line) => rowKey(line[0]!)}>
				{#snippet children(line, index)}
					<!-- 総数と位置を読み上げに渡す。描いている行しか無いので、無いと一覧が数十件に聞こえる。 -->
					<div
						class={browseVirtualLineClass(index === lines.length - 1)}
						style={browseVirtualLineStyle(rowsPerLine)}
					>
						{#each line as row, column (rowKey(row))}
							<div
								role="listitem"
								aria-setsize={rows.length}
								aria-posinset={index * rowsPerLine + column + 1}
								class={browseItemClass()}
							>
								{@render itemRow(row)}
							</div>
						{/each}
					</div>
				{/snippet}
			</WindowVirtualizer>
		{/key}
	</div>
{/if}
