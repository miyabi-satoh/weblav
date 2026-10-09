<script lang="ts">
	import FilterHighlight from '$lib/components/filter-highlight.svelte';
	import LinkIcon from '@lucide/svelte/icons/link';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import { browseLayout } from '$lib/browse-layout.svelte';
	import type { ViewerFile } from '$lib/file-viewer.svelte';
	import { openViewerItem, type ViewerItem } from '$lib/viewer-items';
	import { formatTimeAgo } from '$lib/format';
	import { urlHost } from '$lib/file-kind';
	import { isPlainClick } from '$lib/image-viewer';
	import {
		browseRowClass,
		browseRowSubtitleClass,
		browseRowTextClass,
		browseRowTitleClass,
		linkTileClass
	} from '$lib/list-row';
	import ListRowGlyph from '$lib/components/list-row-glyph.svelte';
	import ListRowIcon from '$lib/components/list-row-icon.svelte';
	import SeparatedText from '$lib/components/separated-text.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import type { components } from '$lib/api/schema';

	type LinkPreview = components['schemas']['LinkPreview'];

	// リンクの行。リストではほかの行と同じ高さで、タイルでは大きい画像のカードにする (→ docs/ui.md「リンクのカード」)。
	// 画像とアイコンはサーバーが取って縮めたもので、見る側の端末は外のサイトにつながない。
	let {
		href,
		title,
		description,
		preview,
		private: isPrivate = false,
		viewer
	}: {
		href: string;
		/** 省くと、ページのタイトル (無ければホスト名) を出す。 */
		title?: string;
		/** 配列なら「·」で区切って並べる。 */
		description?: string | string[] | null;
		preview?: LinkPreview | null;
		private?: boolean;
		/**
		 * 動画サイトの動画なら、ビューアで開くもの (→ docs/ui.md「動画サイトの埋め込み」)。
		 * 修飾キーなしのクリックだけをビューアに回し、ほかは元の URL を新しいタブで開く。
		 */
		viewer?: { file: ViewerFile; items: ViewerItem[] };
	} = $props();

	function handleClick(event: MouseEvent) {
		if (!viewer || !isPlainClick(event)) return;
		event.preventDefault();
		void openViewerItem({ type: 'file', file: viewer.file }, viewer.items);
	}

	let host = $derived(urlHost(href));
	let siteName = $derived(preview?.siteName ?? host);
	let shownTitle = $derived(title ?? preview?.title ?? host);
	let timeAgo = $derived(formatTimeAgo(preview?.publishedAt));

	// 読めなかったときの preview を覚える。取り直しの答えで差し替わったら、同じ URL でも改めて読みに行く。
	// サーバーが消えた画像を取り直し、同じ名前で置き直していることがあるため。
	// 差し替わったかを同一性で見るので、proxy にしない。
	let failedImage = $state.raw<LinkPreview | null>(null);
	let failedIcon = $state.raw<LinkPreview | null>(null);
	let imageUrl = $derived(failedImage === preview ? undefined : (preview?.imageUrl ?? undefined));
	let iconUrl = $derived(failedIcon === preview ? undefined : (preview?.iconUrl ?? undefined));
</script>

<!-- リストでは説明もこの段の末尾に置く。リンクの行だけ3段になって背が高くならないようにするため。 -->
{#snippet meta(withDescription: boolean)}
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
		<span class={['truncate', withDescription && 'max-w-1/2']}>{siteName}</span>
		{#if timeAgo}
			<span aria-hidden="true">·</span>
			<span class="shrink-0">{timeAgo}</span>
		{/if}
		{#if withDescription}
			<span aria-hidden="true">·</span>
			<span class="min-w-0 truncate"><SeparatedText text={description ?? ''} /></span>
		{/if}
	</span>
{/snippet}

{#if browseLayout.tile}
	<a
		{href}
		class={linkTileClass}
		target="_blank"
		rel="external noopener noreferrer"
		onclick={handleClick}
	>
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
			<span class="line-clamp-2 text-base leading-6 wrap-anywhere"
				><FilterHighlight text={shownTitle} /></span
			>
			{#if description}
				<span class="truncate text-xs text-muted-foreground"
					><SeparatedText text={description} /></span
				>
			{/if}
			<span class="flex items-center gap-2 text-xs text-muted-foreground">
				{@render meta(false)}
				{#if !viewer}
					<span class="sr-only">{m.contents_opens_in_new_tab()}</span>
					<ExternalLinkIcon class="ml-auto size-4 shrink-0" strokeWidth={1.8} />
				{/if}
			</span>
		</span>
	</a>
{:else}
	<a
		{href}
		class={browseRowClass()}
		target="_blank"
		rel="external noopener noreferrer"
		onclick={handleClick}
	>
		<ListRowIcon
			icon={LinkIcon}
			thumbnail={imageUrl ? { src: imageUrl, original: href } : undefined}
		/>
		<span class={browseRowTextClass()}>
			<span class={browseRowTitleClass()}><FilterHighlight text={shownTitle} /></span>
			<span class={[browseRowSubtitleClass(), 'flex']}>{@render meta(!!description)}</span>
		</span>
		{#if isPrivate}
			<span class="shrink-0 text-sm text-muted-foreground">{m.contents_visibility_private()}</span>
		{/if}
		<!-- ビューアで開く行には、ファイルの行と同じくグリフを付けない (→ docs/ui.md「PDF・動画・テキストのビューア」)。 -->
		{#if !viewer}
			<ListRowGlyph newTab />
		{/if}
	</a>
{/if}
