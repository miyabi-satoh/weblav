<script lang="ts" module>
	import type { components } from '#lib/api/schema.js';
	import type { LinksFileTarget } from '#lib/links-file.ts';
	import type { RowLocation } from '#lib/row-location.ts';

	type LinkPreview = components['schemas']['LinkPreview'];

	/** 1行分。検索では、ほかの一覧のファイルの行も並ぶので、行ごとに一覧のファイルを持つ。 */
	export type LinksEntryRow = {
		contentId: number;
		target: LinksFileTarget;
		url: string;
		note?: string | null;
		preview?: LinkPreview | null;
		/** 2段目。省くと `note` を出す。検索では、`note` で当たった行だけに `note` を出す。 */
		subtitle?: string | null;
		/** 検索の結果の、どこにあるか。2段目の頭に出す。 */
		location?: RowLocation;
	};
</script>

<script lang="ts">
	import FilterHighlight from '#lib/components/filter-highlight.svelte';
	import { listFilter } from '#lib/list-filter.svelte.ts';
	import ListFilterEmpty from '#lib/components/list-filter-empty.svelte';
	import { linksFileRemoteHref } from '#lib/api/urls.ts';
	import { remoteFile, urlHost } from '#lib/file-kind.ts';
	import type { ViewerFile } from '#lib/file-viewer.svelte.ts';
	import { linkDetailOf, opensLinkDetail } from '#lib/link-detail.svelte.ts';
	import { findViewerItem, viewerItems } from '#lib/viewer-items.ts';
	import { providePreviewSource } from '#lib/preview-url.svelte.ts';
	import type { Track } from '#lib/now-playing.svelte.ts';
	import { videoEmbedUrl } from '#lib/video-embed.ts';
	import ListRowFileLink from '#lib/components/list-row-file-link.svelte';
	import ListRowLink from '#lib/components/list-row-link.svelte';
	import ListRowPlayButton from '#lib/components/list-row-play-button.svelte';
	import ListRowSubtitle from '#lib/components/list-row-subtitle.svelte';
	import { browseItemClass, browseListClass, browseRowTitleClass } from '#lib/list-row.ts';

	// リンクの一覧のファイルの画面 (`/links/[id]`) と検索の結果で使う、一覧の中のリンクの行。
	// URL のファイルと動画サイトの動画は、一覧のコンテンツと同じく、ファイルの行・ビューアーで開く
	// (→ docs/ui.md「URL のファイル」「動画サイトの埋め込み」)。
	let {
		rows: allRows,
		previewOf = (row) => row.preview
	}: {
		rows: LinksEntryRow[];
		/** カードに出す情報。一覧のファイルの画面は、取り直した答えで差し替える。 */
		previewOf?: (row: LinksEntryRow) => LinkPreview | null | undefined;
	} = $props();

	// ページ内の絞り込みが置かれていれば、タイトルで行を絞る (→ docs/ui.md「一覧の絞り込み」)。
	const filter = listFilter();
	let rows = $derived(filter ? filter.apply(allRows, linkTitle) : allRows);

	/** ページのタイトルが取れなければ、ファイルはファイル名、ほかはホスト名 (リンクのカードと同じ)。 */
	function linkTitle(row: LinksEntryRow): string {
		const title = previewOf(row)?.title;
		if (title) return title;
		return remoteOf(row)?.fileName ?? urlHost(row.url);
	}

	/** URL のファイルなら、その種類とファイル名。URL の拡張子で決まらなければ、相手の応答で決めた名前で決める。 */
	function remoteOf(row: LinksEntryRow) {
		return remoteFile(row.url, previewOf(row)?.fileName);
	}

	function remoteHref(row: LinksEntryRow): string {
		return linksFileRemoteHref(row.contentId, row.target, row.url);
	}

	function toTrack(row: LinksEntryRow): Track {
		return { src: remoteHref(row), title: linkTitle(row), originalUrl: row.url };
	}

	function toViewerFile(row: LinksEntryRow): ViewerFile | undefined {
		const embed = videoEmbedUrl(row.url);
		if (embed) {
			return {
				src: embed,
				title: linkTitle(row),
				fileName: '',
				kind: 'embed',
				originalUrl: row.url
			};
		}
		const remote = remoteOf(row);
		if (!remote || remote.kind === 'audio') return undefined;
		return {
			src: remoteHref(row),
			title: linkTitle(row),
			fileName: remote.fileName,
			kind: remote.kind,
			originalUrl: row.url
		};
	}

	let audioQueue = $derived(rows.filter((row) => remoteOf(row)?.kind === 'audio').map(toTrack));
	// URL の `preview` には、リンクの URL を持つ (→ $lib/preview-url.svelte.ts)。並びの位置と違い、
	// 一覧のファイルに行を足しても変わらない。同じ URL が2つ並んでも、開くものは同じ。
	let viewerList = $derived(
		viewerItems(
			rows,
			() => undefined,
			toViewerFile,
			(row) => row.url
		)
	);
	providePreviewSource((key) => findViewerItem(viewerList, key));

	// 詳しい表示で前後へ移る、詳しい表示を出すリンクの並び (→ docs/ui.md「リンクのカード」)。
	// 音声と URL のファイルはファイルの行で出すので入れない。
	let linkDetailRows = $derived(
		rows.filter((row) => !remoteOf(row) && opensLinkDetail(row.url, !!toViewerFile(row)))
	);
	let linkDetails = $derived(
		linkDetailRows.map((row) =>
			linkDetailOf({ href: row.url, description: subtitleOf(row), preview: previewOf(row) })
		)
	);
	let linkDetailIndex = $derived(new Map(linkDetailRows.map((row, index) => [row, index])));

	function subtitleOf(row: LinksEntryRow): string | null | undefined {
		return row.subtitle === undefined ? row.note : row.subtitle;
	}
</script>

{#snippet rowTitle(row: LinksEntryRow)}
	<span class={browseRowTitleClass()}><FilterHighlight text={linkTitle(row)} /></span>
{/snippet}

{#snippet rowSubtitle(row: LinksEntryRow)}
	<ListRowSubtitle location={row.location} text={subtitleOf(row)} />
{/snippet}

{#if filter?.active && rows.length === 0}
	<ListFilterEmpty />
{:else}
	<ul class={browseListClass()}>
		{#each rows as row, index (`${index}:${row.contentId}:${row.url}`)}
			{@const kind = remoteOf(row)?.kind}
			{@const viewerFile = toViewerFile(row)}
			<li class={browseItemClass()}>
				{#if kind === 'audio'}
					<ListRowPlayButton track={toTrack(row)} queue={audioQueue}>
						{@render rowTitle(row)}
						{#snippet subtitle()}{@render rowSubtitle(row)}{/snippet}
					</ListRowPlayButton>
				{:else if kind && viewerFile}
					<ListRowFileLink file={viewerFile} items={viewerList}>
						{@render rowTitle(row)}
						{#snippet subtitle()}{@render rowSubtitle(row)}{/snippet}
					</ListRowFileLink>
				{:else}
					<ListRowLink
						href={row.url}
						description={subtitleOf(row)}
						location={row.location}
						preview={previewOf(row)}
						viewer={viewerFile && { file: viewerFile, items: viewerList }}
						details={{ items: linkDetails, index: linkDetailIndex.get(row) ?? -1 }}
					/>
				{/if}
			</li>
		{/each}
	</ul>
{/if}
