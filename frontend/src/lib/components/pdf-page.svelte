<script lang="ts">
	import type { PDFDocumentProxy, PDFPageProxy, RenderTask } from 'pdfjs-dist/legacy/build/pdf.mjs';
	import * as m from '$lib/paraglide/messages.js';

	// PDF の1ページ (→ pdf-view.svelte)。見えている前後だけを描き、離れたら描いた分を捨てる。
	// iPad の Safari はキャンバスの合計のメモリにも上限があり、全ページを描いたままにすると描けなくなるため。
	let {
		doc,
		root,
		pageNumber,
		width,
		height
	}: {
		doc: PDFDocumentProxy;
		/** ページを並べてスクロールする枠。見えているかは、この枠に対して見る。 */
		root: HTMLElement;
		pageNumber: number;
		/** 表示する大きさ (CSS px)。 */
		width: number;
		height: number;
	} = $props();

	/**
	 * FIX: iOS の Safari はキャンバス1枚の画素数に上限 (約 1,670 万) があり、超えると何も描かない。
	 * 拡大しても、この画素数に収まるまで解像度を下げる。
	 */
	const MAX_CANVAS_PIXELS = 16_000_000;

	let canvas = $state<HTMLCanvasElement>();
	let visible = $state(false);

	/** 描いたページ。見えなくなったら、描くのに使ったデータ (命令の列・展開した画像) も捨てる。 */
	let drawnPage: PDFPageProxy | undefined;

	$effect(() => {
		if (!canvas) return;
		const observer = new IntersectionObserver(
			([entry]) => {
				visible = entry.isIntersecting;
			},
			// 画面1つ分先まで描いておき、スクロールで白いページが見えないようにする。
			// 枠を root に渡さないと、枠の外に隠れた先のページは余白に入っても見えない扱いになる。
			{ root, rootMargin: '100% 0px' }
		);
		observer.observe(canvas);
		return () => observer.disconnect();
	});

	$effect(() => {
		const target = canvas;
		if (!target || !visible) return;
		const cssWidth = width;
		let cancelled = false;
		let task: RenderTask | undefined;
		void (async () => {
			const page = await doc.getPage(pageNumber).catch(() => undefined);
			if (!page || cancelled) return;
			drawnPage = page;
			const base = page.getViewport({ scale: 1 });
			const cssScale = cssWidth / base.width;
			let ratio = window.devicePixelRatio || 1;
			const pixels = base.width * base.height * (cssScale * ratio) ** 2;
			if (pixels > MAX_CANVAS_PIXELS) ratio *= Math.sqrt(MAX_CANVAS_PIXELS / pixels);
			const viewport = page.getViewport({ scale: cssScale * ratio });
			target.width = Math.floor(viewport.width);
			target.height = Math.floor(viewport.height);
			task = page.render({ canvas: target, viewport });
			// 取り消したときも失敗で返るので、捨てる。
			await task.promise.catch(() => undefined);
		})();
		return () => {
			cancelled = true;
			task?.cancel();
		};
	});

	$effect(() => {
		if (!canvas || visible) return;
		// 描いている途中なら、取り消されたときに PDF.js が片付ける。
		drawnPage?.cleanup();
		canvas.width = 0;
		canvas.height = 0;
	});
</script>

<canvas
	bind:this={canvas}
	class="block shrink-0 bg-white shadow-md"
	style:width="{width}px"
	style:height="{height}px"
	aria-label={m.file_viewer_pdf_page({ page: pageNumber })}
></canvas>
