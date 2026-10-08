<script lang="ts">
	import { Dialog as DialogPrimitive } from 'bits-ui';
	import { beforeNavigate } from '$app/navigation';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import XIcon from '@lucide/svelte/icons/x';
	import ZoomInIcon from '@lucide/svelte/icons/zoom-in';
	import ZoomOutIcon from '@lucide/svelte/icons/zoom-out';
	import { fileViewer } from '$lib/file-viewer.svelte';
	import { isMarkdownFileName } from '$lib/markdown-preview';
	import { nowPlaying } from '$lib/now-playing.svelte';
	import OfficeView from '$lib/components/office-view.svelte';
	import PdfView from '$lib/components/pdf-view.svelte';
	import TextView from '$lib/components/text-view.svelte';
	import { Spinner } from '$lib/components/ui/spinner';
	import * as m from '$lib/paraglide/messages.js';

	// PDF・動画・テキストなどをページの上に重ねて表示するビューア (→ docs/ui.md「PDF・動画・テキストのビューア」)。
	// 画面に1つだけ置き (→ routes/+layout.svelte)、一覧の行は `$lib/viewer-items.ts` を通して開く。

	/** PDF の倍率の段階。1 はページを幅に合わせた大きさ。 */
	const ZOOM_STEPS = [1, 1.5, 2, 3];

	let zoomStep = $state(0);
	let failed = $state(false);
	/** Markdown をソースのまま見せる。開いたときはプレビュー (GitHub のファイルの表示と同じ)。 */
	let markdownSource = $state(false);

	let current = $derived(fileViewer.current);

	// 戻るなどで画面を移ったら閉じる。移った先の上に、前の画面のファイルを残さない。
	beforeNavigate(() => {
		fileViewer.open = false;
	});

	// 別のファイルに移ったら、倍率と読めなかった印を戻す。
	$effect.pre(() => {
		void current?.src;
		zoomStep = 0;
		failed = false;
		markdownSource = false;
	});

	// バーのボタン。画像のビューア (PhotoSwipe) と同じく、黒い地の上に白で置く。
	// 見た目は 44px 四方 (→ docs/ui.md「UI 全般」)。
	const barButtonClass =
		'inline-flex size-11 shrink-0 items-center justify-center rounded-md text-white/80 transition-colors hover:bg-white/10 hover:text-white disabled:pointer-events-none disabled:opacity-30';

	function handleKeydown(event: KeyboardEvent) {
		// 動画の再生位置の操作と、拡大した PDF の横のスクロールと重ねない。
		if (event.target instanceof HTMLVideoElement || zoomStep > 0) return;
		// Markdown のプレビューの表は、フォーカスを当てて左右キーで横に送る (→ $lib/manual-markdown.ts)。
		if (event.target instanceof Element && event.target.closest('.table-scroll')) return;
		// Excel の表 (→ docs/ui.md「PDF・動画・テキストのビューア」)。
		if (event.target instanceof Element && event.target.closest('[data-office-view]')) return;
		if (event.key === 'ArrowLeft') fileViewer.previous();
		else if (event.key === 'ArrowRight') fileViewer.next();
		else return;
		event.preventDefault();
	}
</script>

<DialogPrimitive.Root bind:open={fileViewer.open}>
	<DialogPrimitive.Portal>
		<DialogPrimitive.Content
			class="fixed inset-0 z-50 flex flex-col bg-black text-white outline-none"
			onkeydown={handleKeydown}
		>
			{#if current}
				<div class="flex min-h-13 shrink-0 flex-wrap items-center gap-x-1 px-1 md:min-h-14 md:px-2">
					{#if fileViewer.count > 1}
						<span class="shrink-0 px-2 text-sm text-white/80">
							{fileViewer.index + 1} / {fileViewer.count}
						</span>
					{/if}
					<DialogPrimitive.Title class="min-w-0 flex-1 truncate px-2 text-sm">
						{current.title}
					</DialogPrimitive.Title>
					{#if fileViewer.leaving}
						<!-- 隣の画像を読み終えるまで待つ間 (→ docs/ui.md「画像のプレビュー」)。 -->
						<Spinner class="mx-2 size-5 text-white/80" aria-label={m.common_loading()} />
					{/if}
					{#if current.kind === 'pdf' && !failed}
						<button
							type="button"
							class={barButtonClass}
							disabled={zoomStep === 0}
							onclick={() => (zoomStep -= 1)}
						>
							<ZoomOutIcon class="size-5" />
							<span class="sr-only">{m.file_viewer_zoom_out()}</span>
						</button>
						<button
							type="button"
							class={barButtonClass}
							disabled={zoomStep === ZOOM_STEPS.length - 1}
							onclick={() => (zoomStep += 1)}
						>
							<ZoomInIcon class="size-5" />
							<span class="sr-only">{m.file_viewer_zoom_in()}</span>
						</button>
					{/if}
					{#if current.kind === 'text' && isMarkdownFileName(current.fileName) && !failed}
						<!-- GitHub のファイルの表示の「Preview / Code」に倣い、選んでいる側を塗る。
						     スマートフォンの幅では、題名を押し縮めないようバーの2段目に回す。
						     ADR: 閲覧側の切り替えと違い ToggleGroup を使わない。ToggleGroup は左右キーで項目の間を移り、
						     開いた直後にフォーカスが乗るので、左右キーでファイルを送れなくなる。 -->
						<div class="order-last basis-full px-1 pb-2 sm:order-none sm:basis-auto sm:p-0">
							<div
								role="group"
								aria-label={m.file_viewer_markdown_view()}
								class="inline-flex rounded-md bg-white/10 p-0.5"
							>
								{#each [false, true] as source (source)}
									<button
										type="button"
										class={[
											'h-10 rounded px-2.5 text-sm transition-colors',
											markdownSource === source
												? 'bg-white/20 text-white'
												: 'text-white/80 hover:text-white'
										]}
										aria-pressed={markdownSource === source}
										onclick={() => (markdownSource = source)}
									>
										{source ? m.file_viewer_markdown_source() : m.file_viewer_markdown_preview()}
									</button>
								{/each}
							</div>
						</div>
					{/if}
					<!-- API への直リンクか、URL のファイルの元の URL (→ $lib/api/urls.ts)。 -->
					<a
						href={current.originalUrl ?? current.src}
						class={barButtonClass}
						target="_blank"
						rel="external noopener noreferrer"
					>
						<ExternalLinkIcon class="size-5" />
						<span class="sr-only">{m.contents_opens_in_new_tab()}</span>
					</a>
					<DialogPrimitive.Close class={barButtonClass}>
						<XIcon class="size-5" />
						<span class="sr-only">{m.action_close()}</span>
					</DialogPrimitive.Close>
				</div>

				<div class="relative min-h-0 flex-1">
					{#key current.src}
						{#if failed}
							<div
								class="flex size-full flex-col items-center justify-center gap-4 p-4 text-center"
							>
								<p>{m.file_viewer_error()}</p>
								<!-- API への直リンクか、URL のファイルの元の URL (→ $lib/api/urls.ts)。 -->
								<a
									href={current.originalUrl ?? current.src}
									class="text-white underline underline-offset-4"
									target="_blank"
									rel="external noopener noreferrer"
								>
									{m.contents_opens_in_new_tab()}
								</a>
							</div>
						{:else if current.kind === 'pdf'}
							<PdfView
								src={current.src}
								zoom={ZOOM_STEPS[zoomStep]}
								onerror={() => (failed = true)}
							/>
						{:else if current.kind === 'text'}
							<TextView
								src={current.src}
								fileName={current.fileName}
								source={markdownSource}
								onerror={() => (failed = true)}
							/>
						{:else if current.kind === 'office'}
							<OfficeView
								src={current.src}
								fileName={current.fileName}
								onerror={() => (failed = true)}
							/>
						{:else if current.kind === 'embed'}
							<!-- 動画サイトのプレイヤー (→ docs/ui.md「動画サイトの埋め込み」)。音声のプレイヤーは開いた時点で止める。
							     YouTube はリファラーの届かない埋め込みを断るので、オリジンだけは渡す。 -->
							<iframe
								src={current.src}
								title={current.title}
								class="size-full"
								allow="autoplay; encrypted-media; fullscreen; picture-in-picture"
								allowfullscreen
								referrerpolicy="strict-origin-when-cross-origin"
								onload={() => nowPlaying.pause()}
							></iframe>
						{:else if current.kind === 'image'}
							<img
								src={current.src}
								alt={current.title}
								class="size-full object-contain"
								onerror={() => (failed = true)}
							/>
						{:else}
							<!-- 音声のプレイヤーと同時に鳴らさない。
							     字幕は、置かれた動画のファイルしか無いので付けられない。 -->
							<!-- svelte-ignore a11y_media_has_caption -->
							<video
								src={current.src}
								class="size-full object-contain"
								controls
								playsinline
								onplay={() => nowPlaying.pause()}
								onerror={() => (failed = true)}
							></video>
						{/if}
					{/key}

					{#if fileViewer.count > 1}
						<!-- 前後のファイルへ。画像のビューアと同じく、左右の端の中ほどに置く。 -->
						<button
							type="button"
							class={[barButtonClass, 'absolute top-1/2 left-1 -translate-y-1/2 bg-black/50']}
							disabled={fileViewer.index === 0}
							onclick={() => fileViewer.previous()}
						>
							<ChevronLeftIcon class="size-6" />
							<span class="sr-only">{m.file_viewer_previous()}</span>
						</button>
						<button
							type="button"
							class={[barButtonClass, 'absolute top-1/2 right-1 -translate-y-1/2 bg-black/50']}
							disabled={fileViewer.index === fileViewer.count - 1}
							onclick={() => fileViewer.next()}
						>
							<ChevronRightIcon class="size-6" />
							<span class="sr-only">{m.file_viewer_next()}</span>
						</button>
					{/if}
				</div>
			{/if}
		</DialogPrimitive.Content>
	</DialogPrimitive.Portal>
</DialogPrimitive.Root>
