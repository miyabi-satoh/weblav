<script lang="ts">
	import { onMount } from 'svelte';
	import QrCodeIcon from '@lucide/svelte/icons/qr-code';
	import * as m from '$lib/paraglide/messages.js';
	import * as Dialog from '$lib/components/ui/dialog';
	import QrAddress from '$lib/components/qr-address.svelte';
	import { client } from '$lib/api/client';
	import { headerIconActionClass } from '$lib/header-action';

	let open = $state(false);
	/** `null`: 未取得。空文字列: 案内できるアドレスが無い (理由は `unavailable`)。 */
	let url = $state<string | null>(null);
	/** 案内できない理由。`bind`: 他の端末から開けない待ち受け。`name`: PC の名前を案内に使えない。 */
	let unavailable = $state<'bind' | 'name' | 'error'>('error');

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
		} catch {
			url = '';
		}
	}

	onMount(load);
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
			<QrAddress {url} qrAlt={m.connection_info_qr_alt()} />
		{/if}
	</Dialog.Content>
</Dialog.Root>
