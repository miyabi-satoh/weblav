<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { toast } from 'svelte-sonner';
	import ShareIcon from '@lucide/svelte/icons/share';
	import * as m from '$lib/paraglide/messages.js';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import QrAddress from '$lib/components/qr-address.svelte';
	import { client } from '$lib/api/client';
	import { copyText } from '$lib/clipboard';
	import { headerIconActionClass } from '$lib/header-action';
	import { shareableUrl } from '$lib/share-url';

	// 開いているページを、ほかの端末へ渡す (→ docs/ui.md「ページの共有」)。

	type Connection = { mdnsHostname?: string | null; port: number };

	/** `undefined`: まだ聞いていない。`null`: 聞けなかった。 */
	let connection = $state<Connection | null | undefined>(undefined);
	let qrOpen = $state(false);
	/** OS の共有シートを呼べるか。HTTPS か localhost で開いたときだけ (Web Share API の制約)。 */
	let canShare = $state(false);

	onMount(() => {
		canShare = typeof navigator.share === 'function';
	});

	// PC の前 (`localhost`) で開いたときに、PC の名前へ置き換えるため。置き換えが要らなければ使わない。
	async function loadConnection() {
		if (connection !== undefined) return;
		try {
			const { data, response } = await client.GET('/api/v1/connection-info');
			connection = response.ok && data ? data : null;
		} catch {
			connection = null;
		}
	}

	let url = $derived(shareableUrl(page.url, connection ?? undefined));

	async function showQr() {
		await loadConnection();
		qrOpen = true;
	}

	async function shareWithApps() {
		await loadConnection();
		// 渡せるアドレスが無ければ、理由を出す QR の画面へ。
		if (!url) {
			qrOpen = true;
			return;
		}
		try {
			await navigator.share({ title: document.title, url });
		} catch (error) {
			// 利用者が共有シートを閉じたときは、何もしない。
			if (error instanceof DOMException && error.name === 'AbortError') return;
			qrOpen = true;
		}
	}

	async function copyLink() {
		await loadConnection();
		if (!url) {
			qrOpen = true;
			return;
		}
		try {
			await copyText(url);
			toast.success(m.share_copied_toast());
		} catch {
			toast.error(m.common_copy_failed_toast());
		}
	}
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger class={headerIconActionClass}>
		<ShareIcon class="size-5" />
		<span class="sr-only">{m.share_trigger_label()}</span>
	</DropdownMenu.Trigger>
	<DropdownMenu.Content align="end">
		<DropdownMenu.Item onSelect={showQr}>{m.share_qr_menu_item()}</DropdownMenu.Item>
		{#if canShare}
			<DropdownMenu.Item onSelect={shareWithApps}>{m.share_apps_menu_item()}</DropdownMenu.Item>
		{:else}
			<DropdownMenu.Item onSelect={copyLink}>{m.share_copy_menu_item()}</DropdownMenu.Item>
		{/if}
	</DropdownMenu.Content>
</DropdownMenu.Root>

<Dialog.Root bind:open={qrOpen}>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>{m.share_dialog_title()}</Dialog.Title>
			{#if url}
				<Dialog.Description>{m.share_dialog_description()}</Dialog.Description>
			{/if}
		</Dialog.Header>
		{#if connection === undefined}
			<p class="text-sm text-muted-foreground">{m.common_loading()}</p>
		{:else if url}
			<QrAddress {url} qrAlt={m.share_qr_alt()} />
		{:else if connection?.port === 0}
			<p class="text-sm text-muted-foreground">{m.share_bind_local()}</p>
		{:else if connection}
			<p class="text-sm text-muted-foreground">{m.share_unavailable()}</p>
		{:else}
			<p class="text-sm text-muted-foreground">{m.share_load_failed()}</p>
		{/if}
	</Dialog.Content>
</Dialog.Root>
