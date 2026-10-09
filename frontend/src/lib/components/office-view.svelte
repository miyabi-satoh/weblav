<script lang="ts">
	import { Spinner } from '$lib/components/ui/spinner';
	import { fileExtension } from '$lib/file-kind';
	import * as m from '$lib/paraglide/messages.js';

	// ビューアーの Office のファイル (Word・Excel・PowerPoint) の本文 (→ docs/ui.md「PDF・動画・テキストのビューアー」)。
	let { src, fileName, onerror }: { src: string; fileName: string; onerror: () => void } = $props();

	let loading = $state(true);
	let extension = $derived(fileExtension(fileName));

	type Viewer = { load(source: string): Promise<void>; destroy(): void };

	async function createViewer(
		container: HTMLElement,
		extension: string | undefined
	): Promise<Viewer> {
		if (extension === 'docx') {
			const { DocxScrollViewer } = await import('@silurus/ooxml/docx');
			return new DocxScrollViewer(container);
		}
		if (extension === 'pptx') {
			const { PptxScrollViewer } = await import('@silurus/ooxml/pptx');
			return new PptxScrollViewer(container);
		}
		const { XlsxViewer } = await import('@silurus/ooxml/xlsx');
		return new XlsxViewer(container);
	}

	function mount(container: HTMLElement) {
		let viewer: Viewer | undefined;
		let destroyed = false;
		void (async () => {
			try {
				viewer = await createViewer(container, extension);
				if (destroyed) return viewer.destroy();
				await viewer.load(src);
				if (!destroyed) loading = false;
			} catch {
				if (!destroyed) onerror();
			}
		})();
		return () => {
			destroyed = true;
			viewer?.destroy();
		};
	}
</script>

<!-- Excel のビューアーは中に z-index の付いた要素を重ねる。ビューアーの前後のボタンの上に出ないよう、重なりをここで閉じる。 -->
<div class="relative isolate size-full" data-office-view>
	<div class={['size-full', extension === 'xlsx' && !loading && 'bg-white']} {@attach mount}></div>
	{#if loading}
		<div class="absolute inset-0 z-10 flex items-center justify-center">
			<Spinner class="size-8 text-white" aria-label={m.common_loading()} />
		</div>
	{/if}
</div>
