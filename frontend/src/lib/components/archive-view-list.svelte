<script lang="ts" module>
	import type { components } from '#lib/api/schema.js';
	import {
		archiveFileHref,
		archiveItemDownloadHref,
		archiveItemThumbnailHref
	} from '#lib/api/urls.ts';

	type ArchiveViewItem = components['schemas']['ArchiveViewItem'];

	/** 1行分。 */
	export type ArchiveViewRow<T extends ArchiveViewItem = ArchiveViewItem> = {
		archiveId: number;
		item: T;
	};

	/** 行が開く・縮小画像を出す URL。編集ページは、管理用の口を使う。 */
	export type ArchiveItemUrls = {
		download: (archiveId: number, itemId: number) => string;
		thumbnail: (archiveId: number, itemId: number) => string;
		/** 登録先からの相対パスで指したファイル。Markdown のプレビューの相対パスに使う。 */
		file: (archiveId: number, path: string) => string;
	};

	const BROWSE_URLS: ArchiveItemUrls = {
		download: archiveItemDownloadHref,
		thumbnail: archiveItemThumbnailHref,
		file: archiveFileHref
	};
</script>

<script lang="ts" generics="T extends ArchiveViewItem">
	import FilterHighlight from '#lib/components/filter-highlight.svelte';
	import { tick, type Snippet } from 'svelte';
	import type { ContentLocation } from '#lib/content-location.ts';
	import { isAudioFileName, isLinksFileName, viewerFileKind } from '#lib/file-kind.ts';
	import { LinksFileIcon } from '#lib/links-file.ts';
	import type { ViewerFile } from '#lib/file-viewer.svelte.ts';
	import type { Track, TrackSource } from '#lib/now-playing.svelte.ts';
	import type { ViewerImage } from '#lib/image-viewer.ts';
	import { viewerItems, type ViewerItem } from '#lib/viewer-items.ts';
	import { archivePreviewKey, parseArchivePreviewKey } from '#lib/preview-query.ts';
	import { providePreviewSource } from '#lib/preview-url.svelte.ts';
	import { ARCHIVE_CHUNK_SIZE, type ArchivePagedList } from '#lib/archive-paged-list.svelte.ts';
	import { WindowVirtualizer, type WindowVirtualizerHandle } from 'virtua/svelte';
	import { PAGE_LINES, VirtualPages } from '#lib/virtual-pages.svelte.ts';
	import VirtualPageNav from '#lib/components/virtual-page-nav.svelte';
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
	} from '#lib/list-row.ts';
	import ArchiveRowPlaceholder from '#lib/components/archive-row-placeholder.svelte';
	import ListRowSubtitle from '#lib/components/list-row-subtitle.svelte';
	import ListRowIcon from '#lib/components/list-row-icon.svelte';
	import ListRowPlayButton from '#lib/components/list-row-play-button.svelte';
	import ListRowImageLink from '#lib/components/list-row-image-link.svelte';
	import ListRowFileLink from '#lib/components/list-row-file-link.svelte';
	import ListRowDownloadLink from '#lib/components/list-row-download-link.svelte';

	// 一覧の表示 (`/archives/[id]`)・編集ページのファイルのタブで使う、ファイルの行。
	// 押したときの動きと見え方を揃えるため、種類ごとの描き分けをここに集める。
	// 行はサーバーから区切って取る (→ docs/archive.md「画面が見える辺りを取りに行く」)。名前の絞り込みもサーバーで済んでいる。
	let {
		list,
		archiveId,
		linksHref,
		urls = BROWSE_URLS,
		subtitle: subtitleOverride
	}: {
		list: ArchivePagedList<T>;
		archiveId: number;
		linksHref: (row: ArchiveViewRow<T>) => string;
		urls?: ArchiveItemUrls;
		/** 2段目を差し替える (編集ページはパスも出す)。 */
		subtitle?: Snippet<[ArchiveViewRow<T>]>;
	} = $props();

	// 見えている所だけを描くので、行を1段ずつに分けて渡す。タイルは1段に何枚も並ぶ。
	let rowsPerLine = $derived(browseRowsPerLine());
	let totalLines = $derived(Math.ceil(list.total / rowsPerLine));
	const pages = new VirtualPages(() => totalLines);

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
		list.cancelOutside(pages.lines[first] * rowsPerLine, (pages.lines[last] + 1) * rowsPerLine);
	}

	/** 見えている先頭の行の位置。戻ったときに同じ所から出すため、画面が snapshot に残す。 */
	export function firstVisibleIndex(): number {
		if (!virtualizer) return 0;
		const line = pages.lines[virtualizer.findItemIndex(virtualizer.getScrollOffset())] ?? 0;
		return line * rowsPerLine;
	}

	/** `index` 行目へ移る。その区切りを取ってからスクロールする。 */
	export async function scrollToIndex(index: number): Promise<void> {
		if (index <= 0 || index >= list.total) return;
		const line = Math.floor(index / rowsPerLine);
		pages.page = Math.floor(line / PAGE_LINES);
		await list.ensureLoaded(index);
		// ページが替わると一覧を作り直すので、作り直した後の一覧で動かす。
		await tick();
		virtualizer?.scrollToIndex(line - pages.start);
	}

	function rowAt(index: number): ArchiveViewRow<T> | null | undefined {
		const item = list.get(index);
		return item ? { archiveId, item } : item;
	}

	function downloadHref(row: ArchiveViewRow<T>): string {
		return urls.download(row.archiveId, row.item.id);
	}

	/** Markdown のプレビューが相対パスを解決する場所。相対パスの先は、ファイルの番号でなくパスで引く。 */
	function location(row: ArchiveViewRow<T>): ContentLocation {
		return { path: row.item.relPath, src: (path) => urls.file(row.archiveId, path) };
	}

	function toTrack(row: ArchiveViewRow<T>, position: number): Track {
		return { src: downloadHref(row), title: row.item.title, position };
	}

	/** `list` の `start` 行目から `end` 行目の手前までの、手元にある音声の行。`positionOf` は並びの中の位置。 */
	function audioTracksBetween(
		source: ArchivePagedList<T>,
		start: number,
		end: number,
		positionOf: (index: number) => number | undefined = (index) => index
	): Track[] {
		const tracks: Track[] = [];
		for (let i = start; i < end; i++) {
			const item = source.get(i);
			if (!item) continue;
			const row = { archiveId, item };
			const position = positionOf(i);
			if (position === undefined || !isAudioFileName(item.fileName)) continue;
			tracks.push(toTrack(row, position));
		}
		return tracks;
	}

	/** 続けて鳴らす並びが引く一覧。結果が捨てられたら、同じ条件で取り直したものに差し替える。 */
	type ListRef = { source: ArchivePagedList<T> };

	/**
	 * `ref` の一覧の `from` 行目から先の音声を、区切りごとに返す口。音声の無い区切りが続くこともあるので、
	 * 曲が見つかるか終わりまで進む。`positionOf` は、一覧の何行目が並びの中のどの位置か。
	 */
	function chunkedTracks(
		ref: ListRef,
		from: number,
		positionOf: (source: ArchivePagedList<T>, index: number) => number | undefined = (_, index) =>
			index
	): () => Promise<Track[]> {
		let next = from;
		// 最後に並べた区切りの、最後の行。取り直した一覧で、どこから続けるかの目印にする。
		let anchor: T | undefined = (from > 0 ? ref.source.get(from - 1) : undefined) ?? undefined;
		return async () => {
			let renewed = false;
			while (next < ref.source.total) {
				const start = next;
				// 取れなかった区切りも、曲を進めるたびに取り直す。回線が一度切れただけで、続きを止めたままにしないため。
				await ref.source.ensureLoaded(start, true);
				// 聴いている間は一覧を使ったことにならないので、長く聴くと結果が捨てられる。同じ条件で取り直して続ける。
				if (ref.source.expired) {
					// 1回の続きの取得で取り直すのは1回まで。取り直した直後にまた捨てられるなら、取り直し続けずに止める。
					if (renewed) throw new Error('archive list expired again');
					renewed = true;
					const fresh = await ref.source.renewedFrom(start, anchor, (item) => item.id);
					if (!fresh) throw new Error('archive list expired');
					ref.source = fresh.list;
					next = fresh.next;
					continue;
				}
				// 取れなかった区切りは飛ばさない。投げると再生の側が今の並びの終わりで止め、次に曲を替えたときにまた試す。
				if (ref.source.get(start) === undefined) throw new Error('chunk not loaded');
				next = Math.min(
					(Math.floor(start / ARCHIVE_CHUNK_SIZE) + 1) * ARCHIVE_CHUNK_SIZE,
					ref.source.total
				);
				const source = ref.source;
				for (let i = next - 1; i >= start; i--) {
					const item = source.get(i);
					if (item) {
						anchor = item;
						break;
					}
				}
				const tracks = audioTracksBetween(source, start, next, (index) =>
					positionOf(source, index)
				);
				if (tracks.length > 0) return tracks;
			}
			return [];
		};
	}

	/**
	 * `ref` の並びで `from` 行目から続ける口。曲の位置は再生を始めた一覧のものなので、
	 * 絞り込みを変えて一覧が替わっても、始めた一覧から続ける。
	 */
	function orderedSource(ref: ListRef, from: number): TrackSource {
		const after = (track: Track) =>
			orderedSource(ref, track.position === undefined ? ref.source.total : track.position + 1);
		return {
			more: chunkedTracks(ref, from),
			shuffled: ref.source.canShuffle ? () => shuffledSource(ref, after) : undefined,
			after
		};
	}

	/**
	 * 一覧の音声を、サーバーで混ぜた順に返す口 (→ docs/archive.md「音声のシャッフル」)。
	 * 手元に全部の行が無いので、全体から混ぜるのはサーバーに任せる。最初に続きを取るときに混ぜた結果を作る。
	 */
	function shuffledSource(ref: ListRef, after: (track: Track) => TrackSource): TrackSource {
		// 1つの口で作る混ぜた結果は1つ。取り直しのたびに作ると、サーバーに使わない結果が溜まる。
		let more: Promise<(() => Promise<Track[]>) | null> | undefined;
		return {
			more: async () => {
				more ??= shuffledFrom(ref).then((shuffled) =>
					shuffled
						? chunkedTracks({ source: shuffled }, 0, (source, index) => source.positionOf(index))
						: null
				);
				const next = await more.catch((error: unknown) => {
					// 作れなかったら、次に頼まれたときに作り直す。
					more = undefined;
					throw error;
				});
				return next ? next() : [];
			},
			after
		};
	}

	/** `ref` の一覧を混ぜた一覧。一覧の結果が捨てられていたら、取り直してから混ぜる。 */
	async function shuffledFrom(ref: ListRef): Promise<ArchivePagedList<T> | null> {
		const shuffled = await ref.source.shuffled();
		if (shuffled) return shuffled;
		const fresh = await ref.source.renewed();
		if (!fresh) return null;
		ref.source = fresh;
		return fresh.shuffled();
	}

	/**
	 * 続けて鳴らす並び。押した時点で手元にある行から作り (曲の差し替えは操作の中で同期的に行うため)、
	 * 終わりに近づいたら続きの区切りを取って足す (→ docs/archive.md「続けて鳴らす音声とビューアー」)。
	 */
	function audioQueue(index: number): { queue: () => Track[]; more: TrackSource } {
		return {
			queue: () => {
				const [start, end] = list.loadedRange(index);
				return audioTracksBetween(list, start, end);
			},
			more: orderedSource({ source: list }, list.loadedRange(index)[1])
		};
	}

	/**
	 * ビューアーの前後に並べるもの。開くときに前後の区切りを取っておき、その範囲で前後に移る
	 * (→ docs/archive.md「続けて鳴らす音声とビューアー」)。
	 */
	async function viewerListAround(index: number): Promise<ViewerItem[]> {
		// その行のある区切りも取る。URL から開き直したときは、前後だけ取ると行そのものが無く、ビューアーが開かないため。
		await Promise.all([
			list.ensureLoaded(index - ARCHIVE_CHUNK_SIZE),
			list.ensureLoaded(index),
			list.ensureLoaded(index + ARCHIVE_CHUNK_SIZE)
		]);
		// 手元にある行 (消えた・絞り込みで外れた行を除く) から作る。URL の `preview` に持つ key は位置を含むので、位置と一緒に渡す。
		const [start, end] = list.loadedRange(index);
		const rows: { row: ArchiveViewRow<T>; index: number }[] = [];
		for (let i = start; i < end; i++) {
			const row = rowAt(i);
			if (row) rows.push({ row, index: i });
		}
		return viewerItems(
			rows,
			(entry) => toViewerImage(entry.row),
			(entry) => toViewerFile(entry.row),
			(entry) => archivePreviewKey(entry.index, entry.row.item.id)
		);
	}

	// 読み込み直したときは、key の位置の区切りを取ってから探す。位置がずれていても (前にファイルが増えたなど)、
	// 取った前後の区切りにあれば、ファイルの番号で見つける。見つけた行までスクロールし、閉じたときに一覧のその辺りが見えるようにする。
	providePreviewSource(async (key) => {
		const target = parseArchivePreviewKey(key);
		if (!target || list.total === 0) return undefined;
		const items = await viewerListAround(Math.min(target.index, list.total - 1));
		const keys = items.map((item) => parseArchivePreviewKey(item.key ?? ''));
		const index = keys.findIndex((found) => found?.itemId === target.itemId);
		if (index < 0) return undefined;
		void scrollToIndex(keys[index]?.index ?? 0);
		return { items, index };
	});

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

<!-- 1つの行の中身。種類ごとに描き分ける。 -->
{#snippet itemRow(row: ArchiveViewRow<T>, index: number)}
	{@const image = toViewerImage(row)}
	{@const viewerFile = toViewerFile(row)}
	{#if isLinksFileName(row.item.fileName)}
		<div class={browseRowClass()}>
			<ListRowIcon icon={LinksFileIcon} />
			<span class={browseRowTextClass()}>
				<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
				<a href={linksHref(row)} class={browseRowPressClass()}>{@render itemTitle(row)}</a>
				{@render itemSubtitle(row)}
			</span>
		</div>
	{:else if isAudioFileName(row.item.fileName)}
		{@const audio = audioQueue(index)}
		<ListRowPlayButton track={toTrack(row, index)} queue={audio.queue} more={audio.more}>
			{@render itemTitle(row)}
			{#snippet subtitle()}{@render itemSubtitle(row)}{/snippet}
		</ListRowPlayButton>
	{:else if image}
		<ListRowImageLink {image} items={() => viewerListAround(index)}>
			{@render itemTitle(row)}
			{#snippet subtitle()}{@render itemSubtitle(row)}{/snippet}
		</ListRowImageLink>
	{:else if viewerFile}
		<ListRowFileLink file={viewerFile} items={() => viewerListAround(index)}>
			{@render itemTitle(row)}
			{#snippet subtitle()}{@render itemSubtitle(row)}{/snippet}
		</ListRowFileLink>
	{:else}
		<ListRowDownloadLink href={downloadHref(row)} thumbnail={rowThumbnail(row)}>
			{@render itemTitle(row)}
			{#snippet subtitle()}{@render itemSubtitle(row)}{/snippet}
		</ListRowDownloadLink>
	{/if}
{/snippet}

{#snippet pageNav()}
	<VirtualPageNav
		{pages}
		count={PAGE_LINES * rowsPerLine}
		ongo={(next) => void pages.goTo(next, virtualizer)}
	/>
{/snippet}

{#if pages.pageCount > 1}{@render pageNav()}{/if}
<!-- 見えている所だけを描く。アーカイブは数万件になり、全部の行を描くとブラウザーのメモリが尽きて画面が落ちるため。 -->
<div role="list" class={browseVirtualListClass()}>
	<!-- 1段の枚数・ページが変わると段の組み方がすべて変わるので、測った高さを捨てて作り直す。 -->
	{#key `${rowsPerLine}-${pages.page}`}
		<WindowVirtualizer
			bind:this={virtualizer}
			data={pages.lines}
			getKey={(line) => line}
			onscrollend={handleScrollEnd}
		>
			{#snippet children(line, position)}
				<!-- 総数と位置を読み上げに渡す。描いている行しか無いので、無いと一覧が数十件に聞こえる。 -->
				<div
					class={browseVirtualLineClass(position === pages.lines.length - 1)}
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
{#if pages.pageCount > 1}{@render pageNav()}{/if}
