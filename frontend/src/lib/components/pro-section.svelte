<script lang="ts">
	/**
	 * サイト設定の「Pro」の区画 (→ docs/pro.md「結ぶ・確かめる・外す (WebLAV の側)」)。
	 *
	 * 結んでいなければ今の件数と上限を見せ、アカウントと結ぶ。結びかけの間は、窓口の結ぶ画面の
	 * QR コードとボタン、返しのコードの入力欄を出し、ネットで結ばれたかを数秒おきに問い合わせる。
	 * 結んであれば、アカウント・期限を見せ、確かめる・結び直す・外すができる。
	 */
	import { untrack, type Snippet } from 'svelte';
	import QRCode from 'qrcode';
	import { toast } from 'svelte-sonner';
	import { client, type ApiResult } from '$lib/api/client';
	import { errorCode, errorMessage, freeLimitTargetLabel } from '$lib/api/errors';
	import type { components } from '$lib/api/schema';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { formatDate } from '$lib/format';
	import * as m from '$lib/paraglide/messages.js';
	import { getLocale } from '$lib/paraglide/runtime';
	import { qrCardClass, qrImageClass } from '$lib/qr-card';
	import { Button } from '$lib/components/ui/button';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import ConfirmDialog from '$lib/components/confirm-dialog.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import WarningBand from '$lib/components/warning-band.svelte';

	type Pro = components['schemas']['ProResponse'];

	/** 画面からサーバーへ問い合わせる間隔。窓口へはサーバーが5秒より短くない間隔で確かめる。 */
	const POLL_INTERVAL_MS = 3000;
	/** オフラインの PC で、期限が近いと知らせる残りの長さ (→ docs/pro.md「確かめる」)。 */
	const EXPIRING_SECONDS = 7 * 24 * 60 * 60;

	let { initial }: { initial: Pro } = $props();

	let pro = $state(untrack(() => initial));
	/** 窓口の画面も、この画面と同じ言語で開くようにする。 */
	const linkUrl = $derived.by(() => {
		if (!pro.linkUrl) return '';
		const url = new URL(pro.linkUrl);
		url.searchParams.set('lang', getLocale());
		return url.href;
	});
	let linkQr = $state<string | null>(null);
	$effect(() => {
		const url = linkUrl;
		linkQr = null;
		if (url) QRCode.toDataURL(url).then((data) => (linkQr = url === linkUrl ? data : linkQr));
	});
	/** 外したことを窓口へ届けられなかったときの、外した証し。 */
	let release = $state<{ url: string; code: string; qr: string | null } | null>(null);
	let code = $state('');
	/** 直前の問い合わせの失敗。結びかけは続けたまま、枠の中に知らせる。 */
	let pollError = $state<string | null>(null);
	let busy = $state<'link' | 'code' | 'check' | 'cancel' | null>(null);
	let unlinking = $state(false);
	let unlinkOpen = $state(false);
	const errorDialog = new ErrorDialogState();

	const now = () => Math.floor(Date.now() / 1000);
	const expired = $derived(pro.bound && pro.edition === 'free' && !pro.refusal);
	const expiring = $derived(
		pro.edition === 'pro' &&
			pro.permission === 'offline' &&
			pro.expiresAt != null &&
			pro.expiresAt - now() < EXPIRING_SECONDS
	);
	/** 区画の上に出す知らせ。重なるときは、先に手を打つべきもの1つだけを出す。 */
	const notice = $derived.by(() => {
		if (pro.clockBehind) return m.admin_settings_pro_clock_behind();
		if (pro.refusal === 'noPlan') return m.admin_settings_pro_no_plan();
		if (pro.refusal === 'overLimit') return m.admin_settings_pro_over_limit();
		if (expired) return m.admin_settings_pro_expired();
		if (expiring) return m.admin_settings_pro_expiring();
		return null;
	});

	/** 操作を1つ走らせる。失敗はダイアログで知らせる。成功したら区画を新しい状態にする。 */
	async function run(
		kind: NonNullable<typeof busy>,
		call: () => Promise<ApiResult<Pro>>,
		success?: string
	) {
		if (busy) return;
		busy = kind;
		try {
			await errorDialog.attempt(call, ({ data }) => {
				// 成功なのに本文が無いのは想定外。attempt が汎用のエラーとして出す。
				if (!data) throw new Error('empty response');
				const wasPro = pro.edition === 'pro';
				pro = data;
				pollError = null;
				if (success) toast.success(success);
				else if (!wasPro && data.edition === 'pro')
					toast.success(m.admin_settings_pro_linked_toast());
			});
		} finally {
			busy = null;
		}
	}

	function startLink() {
		release = null;
		return run('link', () => client.POST('/api/v1/admin/pro/link'));
	}

	async function submitCode(event: SubmitEvent) {
		event.preventDefault();
		await run('code', () =>
			client.POST('/api/v1/admin/pro/link/code', { body: { code: code.trim() } })
		);
		if (!pro.linkUrl) code = '';
	}

	// 結びかけの間だけ問い合わせる。画面を離れれば止まる (結びかけは残り、開き直すと続く)。
	$effect(() => {
		if (!pro.linkUrl) return;
		let polling = false;
		const timer = setInterval(async () => {
			if (polling || busy) return;
			polling = true;
			try {
				const { data, error, response } = await client.POST('/api/v1/admin/pro/link/poll');
				// 窓口につながらない (502) のは、ネットにつながらない PC では続く。返しのコードの入力に任せる。
				if (!response.ok || !data) {
					pollError =
						errorCode(error) === 'account_server_unavailable' ? null : errorMessage(error);
					return;
				}
				pollError = null;
				const linked = !data.linkUrl;
				pro = data;
				if (linked && data.edition === 'pro') toast.success(m.admin_settings_pro_linked_toast());
			} catch {
				// 一時的に届かなかっただけかもしれないので、次の問い合わせに任せる。
			} finally {
				polling = false;
			}
		}, POLL_INTERVAL_MS);
		return () => clearInterval(timer);
	});

	async function handleUnlink() {
		if (unlinking) return;
		unlinking = true;
		try {
			await errorDialog.attempt(
				async () => {
					// エラーのダイアログを確認のダイアログに重ねないよう、結果を見る前に閉じる。
					try {
						return await client.DELETE('/api/v1/admin/pro');
					} finally {
						unlinkOpen = false;
					}
				},
				async ({ data }) => {
					if (!data) throw new Error('empty response');
					pro = data.pro;
					if (data.releaseUrl && data.releaseCode) {
						release = {
							url: data.releaseUrl,
							code: data.releaseCode,
							qr: await QRCode.toDataURL(data.releaseUrl).catch(() => null)
						};
					}
					toast.success(m.admin_settings_pro_unlinked_toast());
				}
			);
		} finally {
			unlinking = false;
		}
	}
</script>

<!-- 結びかけと、外した証しの枠。どちらも題・手順・QR コードのあとに、それぞれの中身が続く。 -->
{#snippet qrPanel(
	title: string,
	steps: string,
	qrSrc: string | null,
	qrAlt: string,
	live: boolean,
	body: Snippet
)}
	<!-- 性格の違うまとまりなので枠で囲む (→ docs/ui.md「UI 全般」の「余白と区切り」)。 -->
	<div class="flex flex-col gap-6 rounded-md border p-4" aria-live={live ? 'polite' : undefined}>
		<div class="flex flex-col gap-2">
			<p class="text-base font-semibold">{title}</p>
			<p class="text-sm">{steps}</p>
		</div>
		{#if qrSrc}
			<div class="w-fit {qrCardClass}">
				<img src={qrSrc} alt={qrAlt} class={qrImageClass} />
			</div>
		{/if}
		{@render body()}
	</div>
{/snippet}

{#snippet linkingBody()}
	<div class="flex flex-col gap-2">
		<div>
			<Button href={linkUrl} target="_blank" rel="noopener noreferrer"
				>{m.admin_settings_pro_open_account_server()}</Button
			>
		</div>
		<p class="text-sm text-muted-foreground">{m.admin_settings_pro_waiting()}</p>
		{#if pollError}
			<p class="text-sm text-destructive">{pollError}</p>
		{/if}
	</div>
	<form class="flex flex-col gap-2" onsubmit={submitCode}>
		<Field.Field class="max-w-sm">
			<Field.FieldLabel for="pro-link-code">{m.admin_settings_pro_code_label()}</Field.FieldLabel>
			<Input
				id="pro-link-code"
				bind:value={code}
				autocomplete="off"
				autocapitalize="characters"
				spellcheck="false"
			/>
			<Field.FieldDescription>{m.admin_settings_pro_code_description()}</Field.FieldDescription>
		</Field.Field>
		<div class="flex flex-wrap gap-2">
			<LoadingButton
				type="submit"
				variant="outline"
				loading={busy === 'code'}
				disabled={!code.trim()}>{m.admin_settings_pro_code_submit()}</LoadingButton
			>
			<LoadingButton
				variant="ghost"
				loading={busy === 'cancel'}
				onclick={() => run('cancel', () => client.DELETE('/api/v1/admin/pro/link'))}
				>{m.admin_settings_pro_cancel_link()}</LoadingButton
			>
		</div>
	</form>
{/snippet}

{#snippet usageList()}
	<dl class="grid max-w-xs grid-cols-2 gap-x-6 gap-y-1 text-sm">
		{#each pro.usage as usage (usage.target)}
			<dt class="text-muted-foreground">{freeLimitTargetLabel(usage.target)}</dt>
			<dd class="text-right">
				{m.admin_settings_pro_usage({ count: usage.count, limit: usage.limit })}
			</dd>
		{/each}
	</dl>
{/snippet}

{#if pro.edition === 'pro'}
	<p class="text-sm">
		{pro.plan === 'organization'
			? m.admin_settings_pro_current_organization()
			: m.admin_settings_pro_current_personal()}
	</p>
{:else}
	<div class="flex flex-col gap-2">
		<p class="text-sm">{m.admin_settings_pro_free()}</p>
		{@render usageList()}
	</div>
{/if}

{#if notice}
	<!-- 動いていない理由も含め、手を打てば戻る状態なので、失敗の赤ではなく注意の色で出す。 -->
	<WarningBand class="rounded-md border">
		<p class="text-sm leading-relaxed" role="alert">{notice}</p>
	</WarningBand>
{/if}

{#if pro.bound}
	<dl class="grid max-w-md grid-cols-2 gap-x-6 gap-y-1 text-sm">
		<dt class="text-muted-foreground">{m.admin_settings_pro_account_label()}</dt>
		<dd>{pro.email ?? m.admin_settings_pro_account_unknown()}</dd>
		{#if pro.expiresAt != null && pro.edition === 'pro'}
			<dt class="text-muted-foreground">{m.admin_settings_pro_expires_label()}</dt>
			<dd>{formatDate(pro.expiresAt * 1000)}</dd>
		{/if}
		{#if pro.lastCheckedAt != null}
			<dt class="text-muted-foreground">{m.admin_settings_pro_checked_label()}</dt>
			<dd>{formatDate(pro.lastCheckedAt * 1000)}</dd>
		{/if}
	</dl>
{/if}

{#if pro.linkUrl}
	{@render qrPanel(
		m.admin_settings_pro_linking_title(),
		m.admin_settings_pro_linking_steps(),
		linkQr,
		m.admin_settings_pro_qr_alt(),
		true,
		linkingBody
	)}
{:else if !pro.bound}
	<div class="flex flex-col gap-2">
		<div>
			<LoadingButton loading={busy === 'link'} onclick={startLink}
				>{m.admin_settings_pro_link()}</LoadingButton
			>
		</div>
		<p class="text-sm text-muted-foreground">{m.admin_settings_pro_link_description()}</p>
	</div>
{:else}
	<div class="flex flex-wrap gap-2">
		<LoadingButton
			variant="outline"
			loading={busy === 'check'}
			onclick={() =>
				run(
					'check',
					() => client.POST('/api/v1/admin/pro/check'),
					m.admin_settings_pro_checked_toast()
				)}>{m.admin_settings_pro_check_now()}</LoadingButton
		>
		<Button variant="outline" href={pro.accountUrl} target="_blank" rel="noopener noreferrer"
			>{m.admin_settings_pro_open_account_page()}</Button
		>
		<LoadingButton variant="outline" loading={busy === 'link'} onclick={startLink}
			>{m.admin_settings_pro_relink()}</LoadingButton
		>
	</div>
	<div class="flex flex-col gap-2">
		<div>
			<Button variant="outline" onclick={() => (unlinkOpen = true)}
				>{m.admin_settings_pro_unlink()}</Button
			>
		</div>
		<p class="text-sm text-muted-foreground">{m.admin_settings_pro_unlink_description()}</p>
	</div>
{/if}

{#if release}
	{@const { url, code: releaseCode } = release}
	{#snippet body()}
		<div class="flex flex-col gap-2">
			<p class="text-sm text-muted-foreground">
				{m.admin_settings_pro_release_code_label({ url: url.split('?')[0] })}
			</p>
			<p class="text-base font-bold tracking-wider break-all">{releaseCode}</p>
		</div>
	{/snippet}
	{@render qrPanel(
		m.admin_settings_pro_release_title(),
		m.admin_settings_pro_release_steps(),
		release.qr,
		m.admin_settings_pro_release_qr_alt(),
		false,
		body
	)}
{/if}

<ConfirmDialog
	bind:open={unlinkOpen}
	title={m.admin_settings_pro_unlink_confirm_title()}
	busy={unlinking}
	onconfirm={handleUnlink}
	confirmLabel={m.admin_settings_pro_unlink_confirm()}
>
	{m.admin_settings_pro_unlink_confirm_description()}
</ConfirmDialog>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
