<script lang="ts">
	/**
	 * サイト設定の「AI」の区画 (→ docs/ai.md「PC の登録」)。
	 *
	 * 未登録なら PC の登録を始める。登録の途中は、登録を認める画面の QR コードとボタン・確認コードを出し、
	 * 認められたかを数秒おきに問い合わせる。登録済みなら、アカウントと残り・誰がどれだけ使ったかを見せる。
	 *
	 * 状態はページの読み込みでなく、この区画が出てから取る。登録済みのときはサーバーがアカウントへ
	 * 問い合わせるので、アカウントが遅いときに、サイト設定のほかの区画まで待たせないため。
	 */
	import { onMount } from 'svelte';
	import QRCode from 'qrcode';
	import { accountPageHref, formatPercent, usagePercent } from '$lib/ai-account';
	import { aiChat } from '$lib/ai-chat.svelte';
	import { client } from '$lib/api/client';
	import { errorCode, errorMessage, GENERIC_ERROR_MESSAGE } from '$lib/api/errors';
	import { fetchOrNull } from '$lib/api/load';
	import type { components } from '$lib/api/schema';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { formatDateTime, formatNumber } from '$lib/format';
	import * as m from '$lib/paraglide/messages.js';
	import { qrCardClass, qrImageClass } from '$lib/qr-card';
	import { columnFromSmClass, fitColumnClass, truncatingColumnClass } from '$lib/table-columns';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Spinner } from '$lib/components/ui/spinner';
	import * as Table from '$lib/components/ui/table';
	import ConfirmDialog from '$lib/components/confirm-dialog.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';

	type Status = components['schemas']['AiStatusResponse'];
	type UsageItem = components['schemas']['AiUsageItem'];

	/** 画面からサーバーへ問い合わせる間隔。短い間に呼びすぎると、アカウントの側が断る (429)。 */
	const POLL_INTERVAL_MS = 3000;

	/** 取れるまでは `null`。 */
	let status = $state<Status | null>(null);
	let usage = $state<UsageItem[]>([]);
	let loadFailed = $state(false);

	onMount(async () => {
		loadFailed = !(await reload());
	});

	/** アカウント・残り・使った量を読み直す。状態を取れたかを返す。 */
	async function reload(): Promise<boolean> {
		const [next, nextUsage] = await Promise.all([
			fetchOrNull(client.GET('/api/v1/admin/ai')),
			fetchOrNull(client.GET('/api/v1/admin/ai/usage'))
		]);
		if (next) status = next;
		if (nextUsage) usage = nextUsage;
		return next !== null;
	}

	/** 操作の応答で分かった分だけ、状態を入れ替える。 */
	function patch(part: Partial<Status>) {
		if (status) status = { ...status, ...part };
	}

	// 登録が変わったら、簡易チャットを使えるかを確かめ直す。
	let linkState = $derived(status?.state ?? null);
	$effect(() => {
		if (linkState !== null) aiChat.recheck();
	});

	const UNLINKED: Partial<Status> = { state: 'unlinked', linkUrl: null, confirmationCode: null };

	const linkHref = $derived(status?.linkUrl ? accountPageHref(status.linkUrl) : null);
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
	// 登録の途中が、この画面の操作でなく終わった (30分の期限・ほかのタブでの取り消し)。最初のボタンに戻った理由を出す。
	let linkEnded = $state(false);
	/** 直前の問い合わせの失敗。登録の途中は続けたまま、枠の中に知らせる。 */
	let pollError = $state<string | null>(null);
	const errorDialog = new ErrorDialogState();

	async function beginLink() {
		if (busy) return;
		busy = 'link';
		linkEnded = false;
		try {
			let result;
			// エラーのダイアログを確かめのダイアログに重ねないよう、結果を見る前に閉じる。
			try {
				result = await client.POST('/api/v1/admin/ai/link');
			} finally {
				linkOpen = false;
			}
			const { data, error, response } = result;
			if (response.status === 409) {
				// 別のタブや別の管理者が先に登録を終えていた。エラーにせず、今の状態を出す。
				if (!(await reload())) errorDialog.show(errorMessage(error));
				return;
			}
			if (!data) {
				errorDialog.show(response.ok ? GENERIC_ERROR_MESSAGE() : errorMessage(error));
				return;
			}
			pollError = null;
			patch(data);
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
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
				() => patch(UNLINKED)
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
				() => patch({ ...UNLINKED, email: null, remainingPercent: null })
			);
		} finally {
			busy = null;
		}
	}

	// 問い合わせの応答のたびに `status` を入れ替えるので、登録の途中かどうかだけを見る。
	// `status` を直に見ると、応答のたびにタイマーを作り直すことになる。
	const pending = $derived(status?.state === 'pending');

	// 登録の途中の間だけ問い合わせる。画面を離れれば止まる (途中の状態はサーバーに残り、開き直すと続く)。
	$effect(() => {
		if (!pending) return;
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
					if (status?.state !== 'linked') patch(data);
				} else {
					if (data.state === 'unlinked') linkEnded = true;
					patch(data);
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

{#if !status}
	{#if loadFailed}
		<p class="text-sm text-destructive">{m.admin_settings_ai_fetch_failed()}</p>
	{:else}
		<Spinner class="text-muted-foreground" aria-label={m.common_loading()} />
	{/if}
{:else if status.state === 'unlinked'}
	<div class="flex flex-col gap-2">
		{#if linkEnded}
			<p class="text-sm" role="status">{m.admin_settings_ai_link_ended()}</p>
		{/if}
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
		{#if status.confirmationCode}
			<div class="flex flex-col gap-2">
				<p class="text-sm font-semibold">{m.admin_settings_ai_pending_code_label()}</p>
				<!-- 等幅の書体はコードにだけ使うので (→ docs/ui.md「見た目の決めごと」)、字間を空けて読み比べやすくする。 -->
				<p class="text-2xl font-semibold tracking-widest">{status.confirmationCode}</p>
				<p class="text-sm">{m.admin_settings_ai_pending_code_description()}</p>
			</div>
		{/if}
		<div class="flex flex-col gap-2">
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
		<!-- 項目ごとに行を分ける。1文につなぐと、文節での折り返しと項目の切れ目の区別が付かなくなる。 -->
		<div class="flex flex-col gap-3 text-sm">
			<section>
				<h3 class="font-semibold">{m.admin_settings_ai_link_dialog_sent_label()}</h3>
				<ul class="mt-1 flex list-disc flex-col gap-1 pl-5">
					<li>
						{m.admin_settings_ai_link_dialog_sent_axes()}
						<div class="text-muted-foreground">
							{m.admin_settings_ai_link_dialog_sent_axes_purpose()}
						</div>
					</li>
					<li>
						{m.admin_settings_ai_link_dialog_sent_links()}
						<div class="text-muted-foreground">
							{m.admin_settings_ai_link_dialog_sent_links_purpose()}
						</div>
					</li>
					<li>
						{m.admin_settings_ai_link_dialog_sent_chat()}
						<div class="text-muted-foreground">
							{m.admin_settings_ai_link_dialog_sent_chat_purpose()}
						</div>
					</li>
				</ul>
			</section>
			<section>
				<h3 class="font-semibold">{m.admin_settings_ai_link_dialog_destination_label()}</h3>
				<ul class="mt-1 flex list-disc flex-col gap-1 pl-5">
					<li>{m.admin_settings_ai_link_dialog_destination_server()}</li>
					<li>{m.admin_settings_ai_link_dialog_destination_providers()}</li>
				</ul>
			</section>
			<p>
				{m.admin_settings_ai_link_dialog_unlink_note()}
				<a
					href="https://weblav.amiiby.com/privacy/"
					target="_blank"
					rel="noopener noreferrer"
					class="underline underline-offset-4">{m.admin_settings_about_privacy()}</a
				>
			</p>
		</div>
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
