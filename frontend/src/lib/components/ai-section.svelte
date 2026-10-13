<script lang="ts">
	/**
	 * サイト設定の「AI」の区画 (→ docs/ai.md「PC の登録」)。
	 *
	 * 未登録なら PC の登録を始める。登録の途中は、登録を認める画面の QR コードとボタンを出し、
	 * 認められたかを数秒おきに問い合わせる。登録済みなら、アカウントと残り・誰がどれだけ使ったかを見せる。
	 */
	import { untrack } from 'svelte';
	import QRCode from 'qrcode';
	import { toast } from 'svelte-sonner';
	import { accountPageHref, formatPercent, usagePercent } from '$lib/ai-account';
	import { client } from '$lib/api/client';
	import { errorCode, errorMessage } from '$lib/api/errors';
	import type { components } from '$lib/api/schema';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { formatDateTime, formatNumber } from '$lib/format';
	import * as m from '$lib/paraglide/messages.js';
	import { qrCardClass, qrImageClass } from '$lib/qr-card';
	import { columnFromSmClass, fitColumnClass, truncatingColumnClass } from '$lib/table-columns';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Table from '$lib/components/ui/table';
	import ConfirmDialog from '$lib/components/confirm-dialog.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';

	type Status = components['schemas']['AiStatusResponse'];
	type UsageItem = components['schemas']['AiUsageItem'];

	/** 画面からサーバーへ問い合わせる間隔。短い間に呼びすぎると、アカウントの側が断る (429)。 */
	const POLL_INTERVAL_MS = 3000;

	let { initial, initialUsage }: { initial: Status; initialUsage: UsageItem[] } = $props();

	// 操作のたびに応答で入れ直すので、読み込みは初回だけでよい。
	let status = $state(untrack(() => initial));
	let usage = $state(untrack(() => initialUsage));

	const linkHref = $derived(status.linkUrl ? accountPageHref(status.linkUrl) : null);
	let linkQr = $state<string | null>(null);
	$effect(() => {
		const url = linkHref;
		linkQr = null;
		if (!url) return;
		QRCode.toDataURL(url)
			.then((dataUrl) => {
				if (url === linkHref) linkQr = dataUrl;
			})
			.catch(() => {
				// QR コードが作れなくても、「ブラウザーで開く」で登録できる。
			});
	});

	let linkOpen = $state(false);
	let unlinkOpen = $state(false);
	let busy = $state<'link' | 'cancel' | 'unlink' | null>(null);
	/** 直前の問い合わせの失敗。登録の途中は続けたまま、枠の中に知らせる。 */
	let pollError = $state<string | null>(null);
	const errorDialog = new ErrorDialogState();

	/** 登録済みになった後の、アカウント・残り・使った量を読み直す。 */
	async function reload() {
		const [next, nextUsage] = await Promise.all([
			client.GET('/api/v1/admin/ai'),
			client.GET('/api/v1/admin/ai/usage')
		]);
		if (next.data) status = next.data;
		if (nextUsage.data) usage = nextUsage.data;
	}

	async function beginLink() {
		if (busy) return;
		busy = 'link';
		try {
			await errorDialog.attempt(
				async () => {
					// エラーのダイアログを確かめのダイアログに重ねないよう、結果を見る前に閉じる。
					try {
						return await client.POST('/api/v1/admin/ai/link');
					} finally {
						linkOpen = false;
					}
				},
				({ data }) => {
					// 2xx なのに本文が無いのは想定外。汎用のエラーとして出す。
					if (!data) throw new Error('empty response');
					pollError = null;
					status = { ...status, state: data.state, linkUrl: data.linkUrl };
				}
			);
		} finally {
			busy = null;
		}
	}

	async function cancelLink() {
		if (busy) return;
		busy = 'cancel';
		try {
			await errorDialog.attempt(
				() => client.DELETE('/api/v1/admin/ai/link'),
				() => {
					status = { ...status, state: 'unlinked', linkUrl: null };
				}
			);
		} finally {
			busy = null;
		}
	}

	async function unlink() {
		if (busy) return;
		busy = 'unlink';
		try {
			await errorDialog.attempt(
				async () => {
					try {
						return await client.DELETE('/api/v1/admin/ai');
					} finally {
						unlinkOpen = false;
					}
				},
				() => {
					status = { ...status, state: 'unlinked', email: null, remainingPercent: null };
					toast.success(m.admin_settings_ai_unlinked_toast());
				}
			);
		} finally {
			busy = null;
		}
	}

	// 登録の途中の間だけ問い合わせる。画面を離れれば止まる (途中の状態はサーバーに残り、開き直すと続く)。
	$effect(() => {
		if (status.state !== 'pending') return;
		let stopped = false;
		let polling = false;
		const timer = setInterval(async () => {
			if (polling || busy) return;
			polling = true;
			try {
				const { data, error } = await client.POST('/api/v1/admin/ai/link/poll');
				if (stopped) return;
				if (!data) {
					// 呼びすぎと断られたら、黙って次の回を待つ。
					pollError = errorCode(error) === 'ai_rate_limited' ? null : errorMessage(error);
					return;
				}
				pollError = null;
				if (data.state === 'linked') {
					await reload();
					// 読み直せなくても、登録済みになったことは画面に出す。
					if (status.state !== 'linked') status = { ...status, state: 'linked', linkUrl: null };
					toast.success(m.admin_settings_ai_linked_toast());
				} else {
					status = { ...status, state: data.state, linkUrl: data.linkUrl };
				}
			} catch {
				// 一時的に届かなかっただけかもしれないので、次の問い合わせに任せる。
			} finally {
				polling = false;
			}
		}, POLL_INTERVAL_MS);
		return () => {
			stopped = true;
			clearInterval(timer);
		};
	});
</script>

{#if status.state === 'unlinked'}
	<div class="flex flex-col gap-2">
		<div>
			<Button onclick={() => (linkOpen = true)}>{m.admin_settings_ai_link_button()}</Button>
		</div>
		<p class="text-sm text-muted-foreground">{m.admin_settings_ai_link_description()}</p>
	</div>
{:else if status.state === 'pending'}
	<!-- 性格の違うまとまりなので枠で囲む (→ docs/ui.md「UI 全般」の「余白と区切り」)。 -->
	<div class="flex flex-col gap-6 rounded-md border p-4" aria-live="polite">
		<div class="flex flex-col gap-2">
			<p class="text-base font-semibold">{m.admin_settings_ai_pending_title()}</p>
			<p class="text-sm">{m.admin_settings_ai_pending_steps()}</p>
		</div>
		{#if linkQr}
			<div class="w-fit {qrCardClass}">
				<img src={linkQr} alt={m.admin_settings_ai_pending_qr_alt()} class={qrImageClass} />
			</div>
		{/if}
		<div class="flex flex-col gap-2">
			<div class="flex flex-wrap gap-2">
				{#if linkHref}
					<Button href={linkHref} target="_blank" rel="noopener noreferrer"
						>{m.admin_settings_ai_pending_open_button()}</Button
					>
				{/if}
				<LoadingButton variant="outline" loading={busy === 'cancel'} onclick={cancelLink}
					>{m.action_cancel()}</LoadingButton
				>
			</div>
			<p class="text-sm text-muted-foreground">{m.admin_settings_ai_pending_waiting()}</p>
			{#if pollError}
				<p class="text-sm text-destructive">{pollError}</p>
			{/if}
		</div>
	</div>
{:else}
	{#if status.accountUnreachable}
		<p class="text-sm text-destructive">{m.admin_settings_ai_account_unreachable()}</p>
	{:else}
		<dl class="flex flex-col gap-1 text-sm">
			{#if status.email}
				<div class="flex gap-x-6">
					<dt class="w-20 shrink-0 text-muted-foreground">{m.admin_settings_ai_account_label()}</dt>
					<dd class="min-w-0 break-all">{status.email}</dd>
				</div>
			{/if}
			{#if status.remainingPercent != null}
				<div class="flex gap-x-6">
					<dt class="w-20 shrink-0 text-muted-foreground">{m.admin_settings_ai_credit_label()}</dt>
					<dd>
						{m.admin_settings_ai_credit_remaining({
							percent: formatPercent(status.remainingPercent)
						})}
					</dd>
				</div>
			{/if}
		</dl>
	{/if}
	<div class="flex flex-col gap-2">
		<div class="flex flex-wrap gap-2">
			<Button
				variant="outline"
				href={accountPageHref(status.accountUrl)}
				target="_blank"
				rel="noopener noreferrer">{m.admin_settings_ai_account_page_button()}</Button
			>
			<Button variant="outline" onclick={() => (unlinkOpen = true)}
				>{m.admin_settings_ai_unlink_button()}</Button
			>
		</div>
		<p class="text-sm text-muted-foreground">{m.admin_settings_ai_account_page_description()}</p>
	</div>
	<div class="flex flex-col gap-2">
		<h3 class="text-sm font-semibold">{m.admin_settings_ai_usage_heading()}</h3>
		{#if usage.length === 0}
			<p class="text-sm text-muted-foreground">{m.admin_settings_ai_usage_empty()}</p>
		{:else}
			<p class="text-sm text-muted-foreground">{m.admin_settings_ai_usage_description()}</p>
			<Table.Root>
				<Table.Header>
					<Table.Row>
						<Table.Head>{m.admin_users_table_username()}</Table.Head>
						<Table.Head class={[fitColumnClass, 'text-right']}
							>{m.admin_settings_ai_usage_count()}</Table.Head
						>
						<Table.Head class={[fitColumnClass, 'text-right']}
							>{m.admin_settings_ai_usage_amount()}</Table.Head
						>
						<Table.Head class={[columnFromSmClass, fitColumnClass]}
							>{m.admin_settings_ai_usage_last_used()}</Table.Head
						>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					<!-- 消したユーザーの分は id が無く、使った時点の名前でまとまっている。 -->
					{#each usage as row (`${row.userId ?? ''}:${row.username}`)}
						{@const amount = usagePercent(row.used)}
						<Table.Row>
							<Table.Cell class={truncatingColumnClass}>{row.username}</Table.Cell>
							<Table.Cell class={[fitColumnClass, 'text-right']}
								>{formatNumber(row.count)}</Table.Cell
							>
							<Table.Cell class={[fitColumnClass, 'text-right']}>
								{amount.belowMinimum
									? m.admin_settings_ai_usage_amount_below({ percent: amount.text })
									: amount.text}
							</Table.Cell>
							<Table.Cell class={[columnFromSmClass, fitColumnClass, 'text-muted-foreground']}
								>{formatDateTime(row.lastUsedAt)}</Table.Cell
							>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		{/if}
	</div>
{/if}

<!-- 登録すると何を外へ送ることになるかを、始める前に確かめてもらう。 -->
<Dialog.Root bind:open={linkOpen}>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>{m.admin_settings_ai_link_dialog_title()}</Dialog.Title>
			<Dialog.Description>{m.admin_settings_ai_link_dialog_description()}</Dialog.Description>
		</Dialog.Header>
		<dl class="flex flex-col gap-2 text-sm">
			<div>
				<dt class="font-semibold">{m.admin_settings_ai_link_dialog_sent_label()}</dt>
				<dd>{m.admin_settings_ai_link_dialog_sent_axes()}</dd>
			</div>
			<div>
				<dt class="font-semibold">{m.admin_settings_ai_link_dialog_destination_label()}</dt>
				<dd>{m.admin_settings_ai_link_dialog_destination()}</dd>
			</div>
		</dl>
		<Dialog.Footer>
			<Button variant="outline" onclick={() => (linkOpen = false)}>{m.action_cancel()}</Button>
			<LoadingButton loading={busy === 'link'} onclick={beginLink}
				>{m.admin_settings_ai_link_dialog_confirm()}</LoadingButton
			>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>

<ConfirmDialog
	bind:open={unlinkOpen}
	title={m.admin_settings_ai_unlink_confirm_title()}
	busy={busy === 'unlink'}
	onconfirm={unlink}
	confirmLabel={m.admin_settings_ai_unlink_confirm_button()}
>
	{m.admin_settings_ai_unlink_confirm_description()}
</ConfirmDialog>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
