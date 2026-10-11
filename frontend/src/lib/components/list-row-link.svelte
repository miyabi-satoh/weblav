<script lang="ts">
	import FilterHighlight from '$lib/components/filter-highlight.svelte';
	import LinkIcon from '@lucide/svelte/icons/link';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import { browseLayout } from '$lib/browse-layout.svelte';
	import type { ViewerFile } from '$lib/file-viewer.svelte';
	import { openViewerItem, type ViewerItem } from '$lib/viewer-items';
	import { formatTimeAgo } from '$lib/format';
	import { urlHost } from '$lib/file-kind';
	import {
		linkDetail,
		linkDetailOf,
		linkTitleOf,
		opensLinkDetail,
		type LinkDetail
	} from '$lib/link-detail.svelte';
	import { isPlainClick } from '$lib/image-viewer';
	import {
		browseRowClass,
		browseRowPressClass,
		browseRowSubtitleClass,
		browseRowTextClass,
		browseRowTitleClass,
		linkTileClass
	} from '$lib/list-row';
	import ListRowGlyph from '$lib/components/list-row-glyph.svelte';
	import ListRowIcon from '$lib/components/list-row-icon.svelte';
	import RowLocationButton from '$lib/components/row-location.svelte';
	import SeparatedText from '$lib/components/separated-text.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import type { RowLocation } from '$lib/row-location';
	import type { components } from '$lib/api/schema';

	type LinkPreview = components['schemas']['LinkPreview'];

	// リンクの行。リストではほかの行と同じ高さで、タイルでは大きい画像のカードにする (→ docs/ui.md「リンクのカード」)。
	// 画像とアイコンはサーバーが取って縮めたもので、見る側の端末は外のサイトにつながない。
	let {
		href,
		title,
		description,
		location,
		preview,
		private: isPrivate = false,
		viewer,
		details
	}: {
		href: string;
		/** 省くと、ページのタイトル (無ければホスト名) を出す。 */
		title?: string;
		/** 配列なら「·」で区切って並べる。 */
		description?: string | string[] | null;
		/** 検索の結果の、どこにあるか。説明の前に置く。 */
		location?: RowLocation;
		preview?: LinkPreview | null;
		private?: boolean;
		/**
		 * 動画サイトの動画なら、ビューアーで開くもの (→ docs/ui.md「動画サイトの埋め込み」)。
		 * 修飾キーなしのクリックだけをビューアーに回し、ほかは元の URL を新しいタブで開く。
		 */
		viewer?: { file: ViewerFile; items: ViewerItem[] };
		/** 開いた一覧に並ぶ、詳しい表示を出すリンクと、その中のこの行の位置。詳しい表示で前後へ移るのに使う。 */
		details?: { items: LinkDetail[]; index: number };
	} = $props();

	// ビューアーで開かないリンクは、詳しい表示を挟んでから新しいタブで開く (→ docs/ui.md「リンクのカード」)。
	// LAN の URL はサーバーが取りに行かず見せるものが無いので、今までどおり直接開く。
	let opensDetail = $derived(opensLinkDetail(href, !!viewer));

	function handleClick(event: MouseEvent) {
		if (!isPlainClick(event)) return;
		if (viewer) {
			event.preventDefault();
			void openViewerItem({ type: 'file', file: viewer.file }, viewer.items);
		} else if (opensDetail) {
			event.preventDefault();
			if (details && details.index >= 0) linkDetail.show(details.items, details.index);
			else linkDetail.show([linkDetailOf({ href, title, description, preview })], 0);
		}
	}

	let hasDescription = $derived(
		Array.isArray(description) ? description.length > 0 : !!description
	);

	let host = $derived(urlHost(href));
	let siteName = $derived(preview?.siteName ?? host);
	let shownTitle = $derived(linkTitleOf(href, title, preview));
	let timeAgo = $derived(formatTimeAgo(preview?.publishedAt));

	// 読めなかったときの preview を覚える。取り直しの答えで差し替わったら、同じ URL でも改めて読みに行く。
	// サーバーが消えた画像を取り直し、同じ名前で置き直していることがあるため。
	// 差し替わったかを同一性で見るので、proxy にしない。
	let failedImage = $state.raw<LinkPreview | null>(null);
	let failedIcon = $state.raw<LinkPreview | null>(null);
	let imageUrl = $derived(failedImage === preview ? undefined : (preview?.imageUrl ?? undefined));
	let iconUrl = $derived(failedIcon === preview ? undefined : (preview?.iconUrl ?? undefined));
</script>

<!-- リストでは場所と説明もこの段の末尾に置く。リンクの行だけ3段になって背が高くならないようにするため。 -->
{#snippet meta(withDetails: boolean)}
	<!-- サイトのアイコンは、隣のサイト名が何かを伝えるので、読み上げでは飛ばす。 -->
	<span class="flex min-w-0 items-center gap-1.5">
		{#if iconUrl}
			<img
				src={iconUrl}
				alt=""
				loading="lazy"
				decoding="async"
				class="size-4 shrink-0 rounded-xs"
				onerror={() => (failedIcon = preview ?? null)}
			/>
		{/if}
		{#if isPrivate && browseLayout.tile}
			<span class="shrink-0">{m.contents_visibility_private()}</span>
			<span aria-hidden="true">·</span>
		{/if}
		<span class={['truncate', withDetails && (location || hasDescription) && 'max-w-1/2']}
			>{siteName}</span
		>
		{#if timeAgo}
			<span aria-hidden="true">·</span>
			<span class="shrink-0">{timeAgo}</span>
		{/if}
		{#if withDetails}
			{@render rowDetails(true)}
		{/if}
	</span>
{/snippet}

<!-- 場所と説明。1行に収め、場所は親を残して切る。`lead` は、前に並ぶもの (サイト名など) があるか。 -->
{#snippet rowDetails(lead: boolean)}
	{#if location}
		{#if lead}<span aria-hidden="true">·</span>{/if}
		<RowLocationButton {location} clip />
	{/if}
	{#if hasDescription}
		{#if lead || location}<span aria-hidden="true">·</span>{/if}
		<span class="min-w-0 truncate"><SeparatedText text={description ?? ''} /></span>
	{/if}
{/snippet}

<!-- ページの中で開く行 (ビューアー・詳しい表示) には、ファイルの行と同じく新規タブの印を付けない (→ docs/ui.md「PDF・動画・テキストのビューアー」)。 -->
{#snippet titleLink(titleClass: string)}
	<a
		{href}
		class={browseRowPressClass()}
		target="_blank"
		rel="external noopener noreferrer"
		onclick={handleClick}
	>
		<span class={titleClass}><FilterHighlight text={shownTitle} /></span>
		{#if !viewer && !opensDetail}
			<span class="sr-only">{m.contents_opens_in_new_tab()}</span>
		{/if}
	</a>
{/snippet}

{#if browseLayout.tile}
	<div class={linkTileClass}>
		<span class="flex aspect-video w-full items-center justify-center overflow-hidden bg-muted">
			{#if imageUrl}
				<!-- 行のタイトルが何の画像かを伝えるので、読み上げでは飛ばす。 -->
				<img
					src={imageUrl}
					alt=""
					loading="lazy"
					decoding="async"
					class="size-full object-cover"
					onerror={() => (failedImage = preview ?? null)}
				/>
			{:else}
				<LinkIcon class="size-10 text-sub-foreground" strokeWidth={1.5} />
			{/if}
		</span>
		<span class="flex w-full min-w-0 flex-col gap-1 px-3 pt-2.5 pb-3">
			{@render titleLink('line-clamp-2 block text-base leading-6 wrap-anywhere')}
			{#if location || hasDescription}
				<span class="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
					{@render rowDetails(false)}
				</span>
			{/if}
			<span class="flex items-center gap-2 text-xs text-muted-foreground">
				{@render meta(false)}
				{#if !viewer && !opensDetail}
					<ExternalLinkIcon class="ml-auto size-4 shrink-0" strokeWidth={1.8} />
				{/if}
			</span>
		</span>
	</div>
{:else}
	<div class={browseRowClass()}>
		<ListRowIcon
			icon={LinkIcon}
			thumbnail={imageUrl ? { src: imageUrl, original: href } : undefined}
		/>
		<span class={browseRowTextClass()}>
			{@render titleLink(browseRowTitleClass())}
			<span class={[browseRowSubtitleClass(), 'flex']}>{@render meta(true)}</span>
		</span>
		{#if isPrivate}
			<span class="shrink-0 text-sm text-muted-foreground">{m.contents_visibility_private()}</span>
		{/if}
		{#if !viewer && !opensDetail}
			<ListRowGlyph newTab />
		{/if}
	</div>
{/if}
