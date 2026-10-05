<script lang="ts">
	import { onMount } from 'svelte';
	import QRCode from 'qrcode';
	import { toast } from 'svelte-sonner';
	import QrCodeIcon from '@lucide/svelte/icons/qr-code';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import CheckIcon from '@lucide/svelte/icons/check';
	import * as m from '$lib/paraglide/messages.js';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { client } from '$lib/api/client';
	import { copyText } from '$lib/clipboard';
	import { CopiedState } from '$lib/copied-state.svelte';
	import { headerIconActionClass } from '$lib/header-action';
	import { qrCardClass, qrImageClass } from '$lib/qr-card';

	let open = $state(false);
	/** `null`: 未取得。空文字列: 案内できるアドレスが無い (理由は `unavailable`)。 */
	let url = $state<string | null>(null);
	/** 案内できない理由。`bind`: 他の端末から開けない待ち受け。`name`: PC の名前を案内に使えない。 */
	let unavailable = $state<'bind' | 'name' | 'error'>('error');
	let qrDataUrl = $state<string | null>(null);
	const copyFeedback = new CopiedState();

	/**
	 * mDNS のホスト名 (→ docs/architecture.md「LAN からの到達性」) にポートを添えてURLを組み立てる。
	 * ポートはサーバー自身の待ち受けポート (`data.port`) を使う。この画面を開いている
	 * ポート (`location.port`) を使わないのは、`pnpm run dev` (Vite) のように画面と
	 * APIサーバーのポートが別れる場合があるため。
	 */
	async function load() {
		try {
			const { data, response } = await client.GET('/api/v1/connection-info');
			if (!response.ok || !data) {
				url = '';
				return;
			}
			if (!data.mdnsHostname) {
				// 他の端末から開けない待ち受けでは、ポートを 0 で返す (→ api::connection_info)。
				unavailable = data.port === 0 ? 'bind' : 'name';
				url = '';
				return;
			}
			url = `${location.protocol}//${data.mdnsHostname}:${data.port}`;
			qrDataUrl = await QRCode.toDataURL(url);
		} catch {
			url = '';
		}
	}

	onMount(load);

	async function copyUrl() {
		if (!url) return;
		try {
			await copyText(url);
			copyFeedback.show();
		} catch {
			toast.error(m.common_copy_failed_toast());
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Trigger class={headerIconActionClass}>
		<QrCodeIcon class="size-5" />
		<span class="sr-only">{m.connection_info_trigger_label()}</span>
	</Dialog.Trigger>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>{m.connection_info_dialog_title()}</Dialog.Title>
			<!-- アドレスが出ていないときは「このアドレス」が指すものが無いので出さない。 -->
			{#if url}
				<Dialog.Description>{m.connection_info_dialog_description()}</Dialog.Description>
			{/if}
		</Dialog.Header>
		{#if url === null}
			<p class="text-sm text-muted-foreground">{m.common_loading()}</p>
		{:else if url === ''}
			<p class="text-sm text-muted-foreground">
				{#if unavailable === 'bind'}
					{m.connection_info_bind_local()}
				{:else if unavailable === 'name'}
					{m.connection_info_unavailable()}
				{:else}
					{m.connection_info_load_failed()}
				{/if}
			</p>
		{:else}
			<!-- min-w-0: 親(Dialog.Content)は明示の列定義が無いgridで、このdivがそのグリッド
			     アイテムにあたる。無いと、中の長いURL文字列がgridの自動最小サイズ計算に
			     乗って右・下のpaddingまで広がることがある(ブラウザにより挙動差がある)。 -->
			<div class="flex min-w-0 flex-col items-center gap-4">
				{#if qrDataUrl}
					<div class={qrCardClass}>
						<img src={qrDataUrl} alt={m.connection_info_qr_alt()} class={qrImageClass} />
					</div>
				{/if}
				<div class="flex w-full items-start gap-2">
					<!-- font-sans: プロジェクトの方針 (→ docs/ui.md「UI 全般」) で等幅書体は
					     マニュアルのコード表示専用。URLは「控えるアドレス」であり code片ではない
					     ため本文と同じ書体にする。break-all: 省略せず全文を折り返して見せる
					     (手入力したい人が全文を確認できるように)。 -->
					<code
						class="min-w-0 flex-1 rounded border bg-muted px-2 py-1.5 font-sans text-sm break-all"
						>{url}</code
					>
					<!-- アイコンのみ (→ GitHub等の「表示中テキストをコピー」ボタンの作法に合わせる)。
					     成功のフィードバックはアイコンを一時的にチェックマークへ切り替えるだけにし
					     (トーストはこの軽い操作には大げさ)、失敗時だけトーストで知らせる。 -->
					<Button
						variant="outline"
						size="icon-sm"
						onclick={copyUrl}
						aria-label={m.connection_info_copy_button()}
					>
						{#if copyFeedback.copied}
							<CheckIcon />
						{:else}
							<CopyIcon />
						{/if}
					</Button>
				</div>
			</div>
		{/if}
	</Dialog.Content>
</Dialog.Root>
