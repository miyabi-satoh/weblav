<script lang="ts" module>
	import type { components } from '$lib/api/schema';
	import type { LinksFileTarget } from '$lib/links-file';

	type LinkPreview = components['schemas']['LinkPreview'];

	/** 1行分。検索では、ほかの一覧のファイルの行も並ぶので、行ごとに一覧のファイルを持つ。 */
	export type LinksEntryRow = {
		contentId: number;
		target: LinksFileTarget;
		url: string;
		note?: string | null;
		preview?: LinkPreview | null;
		/** 2段目。配列なら「·」で区切って並べる。省くと `note` を出す。 */
		subtitle?: string | string[];
	};
</script>

<script lang="ts">
	import FilterHighlight from '$lib/components/filter-highlight.svelte';
	import { listFilter } from '$lib/list-filter.svelte';
	import ListFilterEmpty from '$lib/components/list-filter-empty.svelte';
	import { linksFileRemoteHref } from '$lib/api/urls';
	import { remoteFileKind, remoteFileName, urlHost } from '$lib/file-kind';
	import type { ViewerFile } from '$lib/file-viewer.svelte';
	import { viewerItems } from '$lib/viewer-items';
	import type { Track } from '$lib/now-playing.svelte';
	import { videoEmbedUrl } from '$lib/video-embed';
	import ListRowFileLink from '$lib/components/list-row-file-link.svelte';
	import ListRowLink from '$lib/components/list-row-link.svelte';
	import ListRowPlayButton from '$lib/components/list-row-play-button.svelte';
	import SeparatedText from '$lib/components/separated-text.svelte';
	import {
		browseItemClass,
		browseListClass,
		browseRowSubtitleClass,
		browseRowTextClass,
		browseRowTitleClass
	} from '$lib/list-row';

	// リンクの一覧のファイルの画面 (`/links/[id]`) と検索の結果で使う、一覧の中のリンクの行。
	// URL のファイルと動画サイトの動画は、一覧のコンテンツと同じく、ファイルの行・ビューアで開く
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
		if (remoteFileKind(row.url)) return remoteFileName(row.url) ?? row.url;
		return urlHost(row.url);
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
		const kind = remoteFileKind(row.url);
		if (!kind || kind === 'audio') return undefined;
		return {
			src: remoteHref(row),
			title: linkTitle(row),
			fileName: remoteFileName(row.url) ?? '',
			kind,
			originalUrl: row.url
		};
	}

	let audioQueue = $derived(rows.filter((row) => remoteFileKind(row.url) === 'audio').map(toTrack));
	let viewerList = $derived(viewerItems(rows, () => undefined, toViewerFile));

	function subtitleOf(row: LinksEntryRow): string | string[] | null | undefined {
		return row.subtitle === undefined ? row.note : row.subtitle;
	}
</script>

{#snippet rowText(row: LinksEntryRow)}
	{@const subtitle = subtitleOf(row)}
	<span class={browseRowTextClass()}>
		<span class={browseRowTitleClass()}><FilterHighlight text={linkTitle(row)} /></span>
		{#if subtitle}
			<span class={browseRowSubtitleClass()}><SeparatedText text={subtitle} /></span>
		{/if}
	</span>
{/snippet}

{#if filter?.active && rows.length === 0}
	<ListFilterEmpty />
{:else}
	<ul class={browseListClass()}>
		{#each rows as row, index (`${index}:${row.contentId}:${row.url}`)}
			{@const kind = remoteFileKind(row.url)}
			{@const viewerFile = toViewerFile(row)}
			<li class={browseItemClass()}>
				{#if kind === 'audio'}
					<ListRowPlayButton track={toTrack(row)} queue={audioQueue}>
						{@render rowText(row)}
					</ListRowPlayButton>
				{:else if kind && viewerFile}
					<ListRowFileLink file={viewerFile} items={viewerList}>
						{@render rowText(row)}
					</ListRowFileLink>
				{:else}
					<ListRowLink
						href={row.url}
						description={subtitleOf(row)}
						preview={previewOf(row)}
						viewer={viewerFile && { file: viewerFile, items: viewerList }}
					/>
				{/if}
			</li>
		{/each}
	</ul>
{/if}
