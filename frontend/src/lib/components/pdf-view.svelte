<script lang="ts">
	import type { PDFDocumentLoadingTask, PDFDocumentProxy } from 'pdfjs-dist/legacy/build/pdf.mjs';
	import PdfPage from '$lib/components/pdf-page.svelte';
	import { Spinner } from '$lib/components/ui/spinner';
	import * as m from '$lib/paraglide/messages.js';

	// ビューアーの PDF の本文 (→ docs/ui.md「PDF・動画・テキストのビューアー」)。ページを縦に並べる。
	// PDF.js の本体と worker は、PDF を開くときに取りに行く。
	// ADR: legacy の版を使う。modern の版は最新のブラウザーにしか無い API (`Map.prototype.getOrInsertComputed` など) を
	// 補わずに呼ぶので、OS を更新できない iPad では1つも開けない。legacy は core-js で補っている。
	let {
		src,
		zoom,
		onerror
	}: {
		src: string;
		/** 幅に合わせた大きさを 1 とする倍率。 */
		zoom: number;
		/** 読めなかったとき。ビューアーが代わりの案内を出す。 */
		onerror: () => void;
	} = $props();

	/** ページを幅に合わせるときの上限 (CSS px)。広い画面で1ページが画面より高くなりすぎないため。 */
	const MAX_FIT_WIDTH = 896;

	/** ページの周りの余白 (`p-4` の左右)。 */
	const PADDING = 32;

	type PageSize = { width: number; height: number };

	let doc = $state.raw<PDFDocumentProxy>();
	let sizes = $state.raw<PageSize[]>([]);
	let containerWidth = $state(0);
	let scroller = $state<HTMLElement>();

	let pageWidth = $derived(
		Math.round(Math.min(Math.max(containerWidth - PADDING, 0), MAX_FIT_WIDTH) * zoom)
	);

	$effect(() => {
		const url = src;
		let cancelled = false;
		let task: PDFDocumentLoadingTask | undefined;
		doc = undefined;
		sizes = [];
		void (async () => {
			try {
				const [pdfjs, { default: workerSrc }, { default: assets }] = await Promise.all([
					import('pdfjs-dist/legacy/build/pdf.mjs'),
					import('pdfjs-dist/legacy/build/pdf.worker.min.mjs?url'),
					import('virtual:pdfjs-assets')
				]);
				pdfjs.GlobalWorkerOptions.workerSrc = workerSrc;
				if (cancelled) return;
				task = pdfjs.getDocument({
					url,
					cMapUrl: assets.cmaps,
					wasmUrl: assets.wasm,
					iccUrl: assets.iccs
				});
				const loaded = await task.promise;
				// 先に全ページの大きさを取り、スクロールの長さを決めておく。描くのは見えてから (→ pdf-page.svelte)。
				const next: PageSize[] = [];
				for (let n = 1; n <= loaded.numPages; n++) {
					const page = await loaded.getPage(n);
					if (cancelled) return;
					const viewport = page.getViewport({ scale: 1 });
					next.push({ width: viewport.width, height: viewport.height });
				}
				if (cancelled) return;
				doc = loaded;
				sizes = next;
			} catch {
				if (!cancelled) onerror();
			}
		})();
		return () => {
			cancelled = true;
			// 読み込みの途中でも、通信と worker を止める。
			void task?.destroy();
		};
	});
</script>

<div class="size-full overflow-auto" bind:this={scroller} bind:clientWidth={containerWidth}>
	{#if doc && scroller && pageWidth > 0}
		<div class="flex w-max min-w-full flex-col items-center gap-4 p-4">
			{#each sizes as size, i (i)}
				<PdfPage
					{doc}
					root={scroller}
					pageNumber={i + 1}
					width={pageWidth}
					height={Math.round((pageWidth * size.height) / size.width)}
				/>
			{/each}
		</div>
	{:else}
		<div class="flex size-full items-center justify-center">
			<Spinner class="size-8 text-white" aria-label={m.common_loading()} />
		</div>
	{/if}
</div>
