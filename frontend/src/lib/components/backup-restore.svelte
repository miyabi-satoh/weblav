<script lang="ts">
	/**
	 * 「バックアップから戻す...」のボタンと確認ダイアログ (→ docs/access.md「バックアップとリストア」)。
	 * サイト設定とセットアップの画面で使う。違うのは送り先 (とトークン) だけなので、要求は呼び出し側が渡す。
	 *
	 * ファイルを選んだらまず受け取らせ (`stage`)、返った目録 (作った日時と版) を確認に出してから戻す。
	 */
	import type { Snippet } from 'svelte';
	import type { ApiResult } from '$lib/api/client';
	import { errorMessage, GENERIC_ERROR_MESSAGE } from '$lib/api/errors';
	import type { components } from '$lib/api/schema';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { formatDateTime } from '$lib/format';
	import * as m from '$lib/paraglide/messages.js';
	import ConfirmDialog from '$lib/components/confirm-dialog.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';

	type Staged = components['schemas']['StagedBackupResponse'];

	type Props = {
		stage: (body: FormData) => Promise<ApiResult<Staged>>;
		restore: (id: string) => Promise<ApiResult>;
		/** 戻し終えたあとの後処理 (ログインへ送るなど)。 */
		onrestored: () => void | Promise<void>;
		/** 404 のときの文言。受け取ったものの期限切れのほか、セットアップではトークンの失効も 404 になる。 */
		notFoundMessage?: string;
		/** 確認ダイアログの説明に足す文 (ログインし直す旨など)。 */
		confirmNote?: Snippet;
		class?: string;
	};

	let {
		stage,
		restore,
		onrestored,
		notFoundMessage = m.backup_restore_expired(),
		confirmNote,
		class: className
	}: Props = $props();

	let fileInput = $state<HTMLInputElement | null>(null);
	let staging = $state(false);
	let restoring = $state(false);
	let staged = $state<Staged | null>(null);
	let confirmOpen = $state(false);
	const errorDialog = new ErrorDialogState();

	function failureMessage(result: ApiResult): string {
		return result.response.status === 404 ? notFoundMessage : errorMessage(result.error);
	}

	async function handleFileChange() {
		const file = fileInput?.files?.[0];
		// 同じファイルを選び直しても change が起きるように空けておく。
		if (fileInput) fileInput.value = '';
		if (!file || staging) return;
		staging = true;

		try {
			const body = new FormData();
			body.append('file', file);
			const result = await stage(body);
			if (!result.response.ok || !result.data) {
				errorDialog.show(failureMessage(result));
				return;
			}
			staged = result.data;
			confirmOpen = true;
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			staging = false;
		}
	}

	async function handleConfirm() {
		if (!staged || restoring) return;
		restoring = true;

		try {
			const result = await restore(staged.id);
			if (!result.response.ok) {
				confirmOpen = false;
				errorDialog.show(failureMessage(result));
				return;
			}
			confirmOpen = false;
			await onrestored();
		} catch {
			confirmOpen = false;
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			restoring = false;
		}
	}
</script>

<div class={className}>
	<LoadingButton variant="outline" loading={staging} onclick={() => fileInput?.click()}
		>{m.backup_restore()}</LoadingButton
	>
	<!-- ボタンから開く。見せないので読み上げからも外す。 -->
	<input
		bind:this={fileInput}
		type="file"
		accept=".zip,application/zip"
		class="hidden"
		tabindex="-1"
		aria-hidden="true"
		onchange={handleFileChange}
	/>
</div>

<ConfirmDialog
	bind:open={confirmOpen}
	title={m.backup_restore_confirm_title()}
	busy={restoring}
	onconfirm={handleConfirm}
	confirmLabel={m.backup_restore_confirm()}
>
	{#if staged}
		{m.backup_restore_confirm_description({
			createdAt: formatDateTime(staged.createdAt * 1000),
			appVersion: staged.appVersion
		})}
	{/if}
	{#if confirmNote}
		<span class="mt-2 block">{@render confirmNote()}</span>
	{/if}
</ConfirmDialog>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
