<script lang="ts" module>
	import type { components } from '$lib/api/schema';
	import {
		archiveFileHref,
		archiveItemDownloadHref,
		archiveItemThumbnailHref
	} from '$lib/api/urls';

	type ArchiveViewItem = components['schemas']['ArchiveViewItem'];

	/** 1行分。 */
	export type ArchiveViewRow<T extends ArchiveViewItem = ArchiveViewItem> = {
		archiveId: number;
		item: T;
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

	/**
	 * 1ページに並べる段の数。要素の高さには上限があり (Firefox で約1,790万 px)、1段 72px で約25万段を超えると
	 * 最後まで届かないので、10万段ごとにページを分ける (→ docs/archive.md「画面が見える辺りを取りに行く」)。
	 */
	const PAGE_LINES = 100_000;
</script>

<script lang="ts" generics="T extends ArchiveViewItem">
	import FilterHighlight from '$lib/components/filter-highlight.svelte';
	import { tick, type Snippet } from 'svelte';
	import type { ContentLocation } from '$lib/content-location';
	import { isAudioFileName, isLinksFileName, viewerFileKind } from '$lib/file-kind';
	import { LinksFileIcon } from '$lib/links-file';
	import type { ViewerFile } from '$lib/file-viewer.svelte';
	import type { Track, TrackSource } from '$lib/now-playing.svelte';
	import type { ViewerImage } from '$lib/image-viewer';
	import { viewerItems, type ViewerItem } from '$lib/viewer-items';
	import { ARCHIVE_CHUNK_SIZE, type ArchivePagedList } from '$lib/archive-paged-list.svelte';
	import { WindowVirtualizer, type WindowVirtualizerHandle } from 'virtua/svelte';
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
	import { formatNumber } from '$lib/format';
	import * as m from '$lib/paraglide/messages.js';
	import { Button } from '$lib/components/ui/button';
	import ArchiveRowPlaceholder from '$lib/components/archive-row-placeholder.svelte';
	import ListRowGlyph from '$lib/components/list-row-glyph.svelte';
	import ListRowSubtitle from '$lib/components/list-row-subtitle.svelte';
	import ListRowIcon from '$lib/components/list-row-icon.svelte';
	import ListRowPlayButton from '$lib/components/list-row-play-button.svelte';
	import ListRowImageLink from '$lib/components/list-row-image-link.svelte';
	import ListRowFileLink from '$lib/components/list-row-file-link.svelte';
	import ListRowDownloadLink from '$lib/components/list-row-download-link.svelte';

	// 一覧の表示 (`/archives/[id]`)・管理画面のアイテムのタブで使う、アーカイブのファイルの行。
	// 押したときの動きと見え方を揃えるため、種類ごとの描き分けをここに集める。
	// 行はサーバーから区切って取る (→ docs/archive.md「画面が見える辺りを取りに行く」)。名前の絞り込みもサーバーで済んでいる。
	let {
		list,
		archiveId,
		linksHref,
		urls = BROWSE_URLS,
		leading,
		trailing,
		subtitle: subtitleOverride,
		isHidden,
		hiddenRow
	}: {
		list: ArchivePagedList<T>;
		archiveId: number;
		linksHref: (row: ArchiveViewRow<T>) => string;
		urls?: ArchiveItemUrls;
		/** 行の前に置くもの (管理画面の選択のチェック)。 */
		leading?: Snippet<[ArchiveViewRow<T>]>;
		/** 行の後ろに置くもの (管理画面の公開の切り替え)。 */
		trailing?: Snippet<[ArchiveViewRow<T>]>;
		/** 2段目を差し替える (管理画面はパスと状態も出す)。 */
		subtitle?: Snippet<[ArchiveViewRow<T>]>;
		/** この画面で非公開にした行か。真なら、行の代わりに `hiddenRow` を出す。 */
		isHidden?: (row: ArchiveViewRow<T>) => boolean;
		hiddenRow?: Snippet<[ArchiveViewRow<T>]>;
	} = $props();

	// 行の部品の中では `trailing` が同名の口を指すので、外で読んでおく。
	let hasTrailing = $derived(trailing !== undefined);

	// 見えている所だけを描くので、行を1段ずつに分けて渡す。タイルは1段に何枚も並ぶ。
	let rowsPerLine = $derived(browseRowsPerLine());
	let totalLines = $derived(Math.ceil(list.total / rowsPerLine));
	let pageCount = $derived(Math.max(1, Math.ceil(totalLines / PAGE_LINES)));
	let page = $state(0);
	// 一覧を取り直したり段の組み方が変わったりして、今のページが無くなったら最後のページにする。
	$effect(() => {
		if (page >= pageCount) page = pageCount - 1;
	});
	let pageStart = $derived(page * PAGE_LINES);
	let lines = $derived(
		Array.from(
			{ length: Math.max(0, Math.min(PAGE_LINES, totalLines - pageStart)) },
			(_, i) => pageStart + i
		)
	);

	function lineIndexes(line: number): number[] {
		const start = line * rowsPerLine;
		return Array.from({ length: Math.min(rowsPerLine, list.total - start) }, (_, i) => start + i);
	}

	let virtualizer = $state<WindowVirtualizerHandle | undefined>();

	/** 弾くようにスクロールして見えなくなった区切りの要求をやめる。 */
	function handleScrollEnd() {
		if (!virtualizer) return;
		const top = virtualizer.getScrollOffset();
		const first = virtualizer.findItemIndex(top);
		const last = virtualizer.findItemIndex(top + virtualizer.getViewportSize());
		list.cancelOutside(lines[first] * rowsPerLine, (lines[last] + 1) * rowsPerLine);
	}

	/** 見えている先頭の行の位置。戻ったときに同じ所から出すため、画面が snapshot に残す。 */
	export function firstVisibleIndex(): number {
		if (!virtualizer) return 0;
		const line = lines[virtualizer.findItemIndex(virtualizer.getScrollOffset())] ?? 0;
		return line * rowsPerLine;
	}

	/** `index` 行目へ移る。その区切りを取ってからスクロールする。 */
	export async function scrollToIndex(index: number): Promise<void> {
		if (index <= 0 || index >= list.total) return;
		const line = Math.floor(index / rowsPerLine);
		page = Math.floor(line / PAGE_LINES);
		await list.ensureLoaded(index);
		// ページが替わると一覧を作り直すので、作り直した後の一覧で動かす。
		await tick();
		virtualizer?.scrollToIndex(line - page * PAGE_LINES);
	}

	/** ページを移り、その先頭から見せる。下のボタンで移ったときに、新しいページの途中から始まらないように。 */
	async function goToPage(next: number) {
		page = next;
		await tick();
		virtualizer?.scrollToIndex(0);
	}

	function rowAt(index: number): ArchiveViewRow<T> | null | undefined {
		const item = list.get(index);
		return item ? { archiveId, item } : item;
	}

	/** `index` 行目の周りの、手元にある行 (消えた・非公開にした行を除く)。 */
	function loadedRowsAround(index: number): ArchiveViewRow<T>[] {
		const [start, end] = list.loadedRange(index);
		return rowsBetween(start, end);
	}

	function rowsBetween(start: number, end: number): ArchiveViewRow<T>[] {
		const rows: ArchiveViewRow<T>[] = [];
		for (let i = start; i < end; i++) {
			const row = rowAt(i);
			if (row && !isHidden?.(row)) rows.push(row);
		}
		return rows;
	}

	function downloadHref(row: ArchiveViewRow<T>): string {
		return urls.download(row.archiveId, row.item.id);
	}

	/** Markdown のプレビューが相対パスを解決する場所。相対パスの先は、アイテムの番号でなくパスで引く。 */
	function location(row: ArchiveViewRow<T>): ContentLocation {
		return { path: row.item.relPath, src: (path) => urls.file(row.archiveId, path) };
	}

	function toTrack(row: ArchiveViewRow<T>): Track {
		return { src: downloadHref(row), title: row.item.title };
	}

	function audioTracks(rows: ArchiveViewRow<T>[]): Track[] {
		return rows.filter((row) => isAudioFileName(row.item.fileName)).map(toTrack);
	}

	/**
	 * 続けて鳴らす並び。押した時点で手元にある行から作り (曲の差し替えは操作の中で同期的に行うため)、
	 * 終わりに近づいたら続きの区切りを取って足す (→ docs/archive.md「続けて鳴らす音声とビューアー」)。
	 */
	function audioQueue(index: number): { queue: () => Track[]; more: TrackSource } {
		let next = list.loadedRange(index)[1];
		return {
			queue: () => audioTracks(loadedRowsAround(index)),
			more: {
				more: async () => {
					// 音声の無い区切りが続くこともあるので、曲が見つかるか終わりまで進む。
					while (next < list.total) {
						const start = next;
						await list.ensureLoaded(start);
						// 取れなかった区切りは飛ばさない。投げると再生の側が今の並びの終わりで止め、次に曲を替えたときにまた試す。
						if (list.get(start) === undefined) throw new Error('chunk not loaded');
						next = Math.min(
							(Math.floor(start / ARCHIVE_CHUNK_SIZE) + 1) * ARCHIVE_CHUNK_SIZE,
							list.total
						);
						const tracks = audioTracks(rowsBetween(start, next));
						if (tracks.length > 0) return tracks;
					}
					return [];
				}
			}
		};
	}

	/**
	 * ビューアーの前後に並べるもの。開くときに前後の区切りを取っておき、その範囲で前後に移る
	 * (→ docs/archive.md「続けて鳴らす音声とビューアー」)。
	 */
	async function viewerListAround(index: number): Promise<ViewerItem[]> {
		await Promise.all([
			list.ensureLoaded(index - ARCHIVE_CHUNK_SIZE),
			list.ensureLoaded(index + ARCHIVE_CHUNK_SIZE)
		]);
		return viewerItems(loadedRowsAround(index), toViewerImage, toViewerFile);
	}

	/** 画像の行なら、ビューアーに渡す形。大きさが分からない (読めない) 画像は普通のファイルの行にする。 */
	function toViewerImage(row: ArchiveViewRow<T>): ViewerImage | undefined {
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
	function rowThumbnail(row: ArchiveViewRow<T>): { src: string; original: string } | undefined {
		if (!row.item.thumbnail) return undefined;
		return {
			src: urls.thumbnail(row.archiveId, row.item.id),
			original: downloadHref(row)
		};
	}

	/** PDF・動画・テキストなど、ビューアーで開く行なら、ビューアーに渡す形 (→ docs/ui.md「PDF・動画・テキストのビューアー」)。 */
	function toViewerFile(row: ArchiveViewRow<T>): ViewerFile | undefined {
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
</script>

{#snippet itemTitle(row: ArchiveViewRow)}
	<span class={browseRowTitleClass()}><FilterHighlight text={row.item.title} /></span>
{/snippet}

<!-- 2段目は、アーカイブの一覧では同じタイトルが並んだときに見分けるための軸の値 (→ docs/ui.md「アーカイブの一覧画面」)。 -->
{#snippet itemSubtitle(row: ArchiveViewRow<T>)}
	{#if subtitleOverride}
		{@render subtitleOverride(row)}
	{:else}
		<ListRowSubtitle text={row.item.subtitle} />
	{/if}
{/snippet}

<!-- 行の部品の同名の口へ渡すため、別名で包む。 -->
{#snippet leadingSlot(row: ArchiveViewRow<T>)}{@render leading?.(row)}{/snippet}
{#snippet trailingSlot(row: ArchiveViewRow<T>)}{@render trailing?.(row)}{/snippet}

<!-- 1つの行の中身。種類ごとに描き分ける。 -->
{#snippet itemRow(row: ArchiveViewRow<T>, index: number)}
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
		{@const audio = audioQueue(index)}
		<ListRowPlayButton track={toTrack(row)} queue={audio.queue} more={audio.more}>
			{@render itemTitle(row)}
			{#snippet subtitle()}{@render itemSubtitle(row)}{/snippet}
			{#snippet leading()}{@render leadingSlot(row)}{/snippet}
			{#snippet trailing()}{@render trailingSlot(row)}{/snippet}
		</ListRowPlayButton>
	{:else if image}
		<ListRowImageLink {image} items={() => viewerListAround(index)}>
			{@render itemTitle(row)}
			{#snippet subtitle()}{@render itemSubtitle(row)}{/snippet}
			{#snippet leading()}{@render leadingSlot(row)}{/snippet}
			{#snippet trailing()}{@render trailingSlot(row)}{/snippet}
		</ListRowImageLink>
	{:else if viewerFile}
		<ListRowFileLink file={viewerFile} items={() => viewerListAround(index)}>
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

{#snippet pageNav()}
	<div class="flex flex-wrap items-center justify-center gap-3 py-4">
		<Button
			type="button"
			variant="outline"
			disabled={page === 0}
			onclick={() => void goToPage(page - 1)}
		>
			{m.archive_view_previous_page({ count: formatNumber(PAGE_LINES * rowsPerLine) })}
		</Button>
		<span class="text-sm text-muted-foreground">
			{m.archive_view_page_position({ page: page + 1, pages: pageCount })}
		</span>
		<Button
			type="button"
			variant="outline"
			disabled={page >= pageCount - 1}
			onclick={() => void goToPage(page + 1)}
		>
			{m.archive_view_next_page({ count: formatNumber(PAGE_LINES * rowsPerLine) })}
		</Button>
	</div>
{/snippet}

{#if pageCount > 1}{@render pageNav()}{/if}
<!-- 見えている所だけを描く。アーカイブは数万件になり、全部の行を描くとブラウザーのメモリが尽きて画面が落ちるため。 -->
<div role="list" class={browseVirtualListClass()}>
	<!-- 1段の枚数・ページが変わると段の組み方がすべて変わるので、測った高さを捨てて作り直す。 -->
	{#key `${rowsPerLine}-${page}`}
		<WindowVirtualizer
			bind:this={virtualizer}
			data={lines}
			getKey={(line) => line}
			onscrollend={handleScrollEnd}
		>
			{#snippet children(line, position)}
				<!-- 総数と位置を読み上げに渡す。描いている行しか無いので、無いと一覧が数十件に聞こえる。 -->
				<div
					class={browseVirtualLineClass(position === lines.length - 1)}
					style={browseVirtualLineStyle(rowsPerLine)}
				>
					{#each lineIndexes(line) as index (index)}
						{@const row = rowAt(index)}
						<div
							role="listitem"
							aria-setsize={list.total}
							aria-posinset={index + 1}
							class={browseItemClass()}
						>
							{#if row}
								{@render itemRow(row, index)}
							{:else}
								<ArchiveRowPlaceholder {list} {index} missing={row === null} />
							{/if}
						</div>
					{/each}
				</div>
			{/snippet}
		</WindowVirtualizer>
	{/key}
</div>
{#if pageCount > 1}{@render pageNav()}{/if}
