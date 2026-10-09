<script lang="ts">
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import { beforeNavigate } from '$app/navigation';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Button, buttonVariants } from '$lib/components/ui/button';
	import SeparatedText from '$lib/components/separated-text.svelte';
	import { urlHost } from '$lib/file-kind';
	import { formatTimeAgo } from '$lib/format';
	import { linkDetail } from '$lib/link-detail.svelte';
	import * as m from '$lib/paraglide/messages.js';

	// リンクの詳しい表示。一覧のカードでは縮んでいる画像・題・説明を全部見せてから、新しいタブで開かせる
	// (→ docs/ui.md「リンクのカード」)。
	let detail = $derived(linkDetail.current);
	let preview = $derived(detail?.preview);
	let siteName = $derived(preview?.siteName ?? (detail ? urlHost(detail.href) : ''));
	let timeAgo = $derived(formatTimeAgo(preview?.publishedAt));

	// 読めなかった画像は隠す。開き直したり前後へ移ったりしたら、改めて読みに行く。
	let failedImage = $state.raw<string | null>(null);
	let failedIcon = $state.raw<string | null>(null);
	$effect.pre(() => {
		void linkDetail.open;
		void linkDetail.current;
		failedImage = null;
		failedIcon = null;
	});

	// 戻るなどで画面を移ったら閉じる。移った先の上に、前の画面のリンクを残さない (ファイルのビューアと同じ)。
	beforeNavigate(() => {
		linkDetail.open = false;
	});
	let imageUrl = $derived(preview?.imageUrl !== failedImage ? preview?.imageUrl : null);
	let iconUrl = $derived(preview?.iconUrl !== failedIcon ? preview?.iconUrl : null);

	// 登録するときに、説明をページの説明から埋めることがある (`POST /contents/link-metadata`)。同じ文を2回出さない。
	let pageDescription = $derived(
		preview?.description && preview.description !== detail?.description ? preview.description : null
	);

	let openButton = $state<HTMLAnchorElement | null>(null);

	// 端に着いて押したボタンが押せなくなると、フォーカスが外れて左右キーが届かなくなる。開くボタンへ移す。
	function previous() {
		linkDetail.previous();
		if (linkDetail.index === 0) openButton?.focus();
	}

	function next() {
		linkDetail.next();
		if (linkDetail.index === linkDetail.count - 1) openButton?.focus();
	}

	// 左右キーで前後のリンクへ移る (ファイルのビューアと同じ)。
	function handleKeydown(event: KeyboardEvent) {
		if (event.key === 'ArrowLeft') previous();
		else if (event.key === 'ArrowRight') next();
		else return;
		event.preventDefault();
	}
</script>

<Dialog.Root bind:open={linkDetail.open}>
	<!-- 開いたら「新しいタブで開く」にフォーカスを置く。開くだけの人が Enter 1回で済むように。 -->
	<Dialog.Content
		class="sm:max-w-xl"
		onkeydown={handleKeydown}
		onOpenAutoFocus={(event) => {
			event.preventDefault();
			openButton?.focus();
		}}
	>
		{#if detail}
			<Dialog.Header>
				{#if linkDetail.count > 1}
					<p class="text-muted-foreground">{linkDetail.index + 1} / {linkDetail.count}</p>
				{/if}
				<!-- 右上の閉じるボタンの下に、長い題が潜らないよう空ける。題は2行以上になることがあるので、行の間も空ける。 -->
				<Dialog.Title class="pr-8 leading-snug wrap-anywhere">{detail.title}</Dialog.Title>
				<!-- サイトのアイコンは、隣のサイト名が何かを伝えるので、読み上げでは飛ばす。 -->
				<p class="flex min-w-0 items-center gap-1.5 text-muted-foreground">
					{#if iconUrl}
						<img
							src={iconUrl}
							alt=""
							class="size-4 shrink-0 rounded-xs"
							onerror={() => (failedIcon = iconUrl ?? null)}
						/>
					{/if}
					<span class="truncate">{siteName}</span>
					{#if timeAgo}
						<span aria-hidden="true">·</span>
						<span class="shrink-0">{timeAgo}</span>
					{/if}
				</p>
			</Dialog.Header>
			{#if imageUrl}
				<!-- 題が何の画像かを伝えるので、読み上げでは飛ばす。題の下に置くのは、右上の閉じるボタンが画像に重なって見えなくならないため。 -->
				<img
					src={imageUrl}
					alt=""
					decoding="async"
					class="max-h-96 w-full rounded-md bg-muted object-contain"
					onerror={() => (failedImage = imageUrl ?? null)}
				/>
			{/if}
			{#if detail.description || pageDescription}
				<div class="flex flex-col gap-2 wrap-anywhere">
					{#if detail.description}
						<p><SeparatedText text={detail.description} /></p>
					{/if}
					{#if pageDescription}
						<p class="text-muted-foreground">{pageDescription}</p>
					{/if}
				</div>
			{/if}
			<p class="text-xs break-all text-muted-foreground">{detail.href}</p>
			<!-- 前後へ移れるときは、前後のボタンを左に、開くボタンを右に置く。1件だけなら、ほかのダイアログと同じ並べ方にする。 -->
			<Dialog.Footer class={[linkDetail.count > 1 && 'flex-row flex-wrap items-center']}>
				{#if linkDetail.count > 1}
					<Button
						variant="outline"
						size="icon-lg"
						disabled={linkDetail.index === 0}
						onclick={previous}
					>
						<ChevronLeftIcon />
						<span class="sr-only">{m.link_detail_previous_button()}</span>
					</Button>
					<Button
						variant="outline"
						size="icon-lg"
						disabled={linkDetail.index === linkDetail.count - 1}
						onclick={next}
					>
						<ChevronRightIcon />
						<span class="sr-only">{m.link_detail_next_button()}</span>
					</Button>
				{/if}
				<a
					bind:this={openButton}
					href={detail.href}
					target="_blank"
					rel="external noopener noreferrer"
					class={[buttonVariants(), linkDetail.count > 1 && 'ml-auto']}
					onclick={() => (linkDetail.open = false)}
				>
					<ExternalLinkIcon data-icon="inline-start" />
					{m.link_detail_open_button()}
				</a>
			</Dialog.Footer>
		{/if}
	</Dialog.Content>
</Dialog.Root>
