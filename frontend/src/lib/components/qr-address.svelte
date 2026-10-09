<script lang="ts">
	import QRCode from 'qrcode';
	import { toast } from 'svelte-sonner';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import CheckIcon from '@lucide/svelte/icons/check';
	import * as m from '$lib/paraglide/messages.js';
	import { Button } from '$lib/components/ui/button';
	import { copyText } from '$lib/clipboard';
	import { CopiedState } from '$lib/copied-state.svelte';
	import { qrCardClass, qrImageClass } from '$lib/qr-card';

	// ほかの端末で開くアドレスの、QR コードと控えるための全文・コピー (ページの共有)。
	let { url, qrAlt }: { url: string; qrAlt: string } = $props();

	let qrDataUrl = $state<string | null>(null);
	const copyFeedback = new CopiedState();

	$effect(() => {
		const target = url;
		qrDataUrl = null;
		QRCode.toDataURL(target)
			.then((dataUrl) => {
				if (target === url) qrDataUrl = dataUrl;
			})
			.catch(() => {
				// QR コードが作れなくても、アドレスの全文とコピーで渡せる。
			});
	});

	async function copyUrl() {
		try {
			await copyText(url);
			copyFeedback.show();
		} catch {
			toast.error(m.common_copy_failed_toast());
		}
	}
</script>

<!-- min-w-0: 親(Dialog.Content)は明示の列定義が無いgridで、このdivがそのグリッド
     アイテムにあたる。無いと、中の長いURL文字列がgridの自動最小サイズ計算に
     乗って右・下のpaddingまで広がることがある(ブラウザにより挙動差がある)。 -->
<div class="flex min-w-0 flex-col items-center gap-4">
	{#if qrDataUrl}
		<div class={qrCardClass}>
			<img src={qrDataUrl} alt={qrAlt} class={qrImageClass} />
		</div>
	{/if}
	<div class="flex w-full items-start gap-2">
		<!-- font-sans: プロジェクトの方針 (→ docs/ui.md「UI 全般」) で等幅書体は
		     マニュアルのコード表示専用。URLは「控えるアドレス」であり code片ではない
		     ため本文と同じ書体にする。break-all: 省略せず全文を折り返して見せる
		     (手入力したい人が全文を確認できるように)。 -->
		<code class="min-w-0 flex-1 rounded border bg-muted px-2 py-1.5 font-sans text-sm break-all"
			>{url}</code
		>
		<!-- アイコンのみ (→ GitHub等の「表示中テキストをコピー」ボタンの作法に合わせる)。
		     成功のフィードバックはアイコンを一時的にチェックマークへ切り替えるだけにし
		     (トーストはこの軽い操作には大げさ)、失敗時だけトーストで知らせる。 -->
		<Button variant="outline" size="icon-sm" onclick={copyUrl} aria-label={m.share_copy_button()}>
			{#if copyFeedback.copied}
				<CheckIcon />
			{:else}
				<CopyIcon />
			{/if}
		</Button>
	</div>
</div>
