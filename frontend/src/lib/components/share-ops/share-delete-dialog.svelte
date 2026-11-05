<script lang="ts">
	/**
	 * 共有の中のものの削除 (→ docs/ui.md「共有の中の画面」)。開くと先にサーバーに確かめ、
	 * ごみ箱へ送れる場所かで文言を分け (送れなければ「元に戻せません」)、消すと開けなくなるホームのコンテンツを並べる。
	 * 消し始めたら裏で続く処理になるので、進み具合はルートのレイアウトの欄に任せて閉じる。
	 */
	import { client } from '$lib/api/client';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import { jobLabel, shareJobs } from '$lib/share-jobs.svelte';
	import type { ShareTarget } from '$lib/share-ops';
	import type { components } from '$lib/api/schema';
	import ConfirmDialog from '$lib/components/confirm-dialog.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';

	type DeleteCheck = components['schemas']['DeleteCheckResponse'];

	let {
		target = $bindable(),
		shareId,
		ondone
	}: {
		/** 消すもの。入れると確かめてから開き、閉じると `null` に戻す。 */
		target: ShareTarget | null;
		shareId: number;
		ondone: () => void | Promise<void>;
	} = $props();

	let checked = $state<{ target: ShareTarget; result: DeleteCheck } | null>(null);
	let busy = $state(false);
	const errorDialog = new ErrorDialogState();

	// 対象が入ったら確かめる。確かめられなければ、理由を出して閉じる。
	$effect(() => {
		const current = target;
		if (current === null) return;
		let stale = false;
		void (async () => {
			const result = await errorDialog.attempt(() =>
				client.POST('/api/v1/shares/{id}/delete/check', {
					params: { path: { id: shareId } },
					body: { paths: [current.path] }
				})
			);
			if (stale) return;
			if (result?.data == null) {
				target = null;
				return;
			}
			checked = { target: current, result: result.data };
		})();
		return () => {
			stale = true;
		};
	});

	function close() {
		target = null;
		checked = null;
	}

	async function confirm() {
		if (checked === null || busy) return;
		const { target: doomed, result } = checked;
		busy = true;
		try {
			const started = await errorDialog.attempt(() =>
				client.POST('/api/v1/shares/{id}/delete', {
					params: { path: { id: shareId } },
					body: { paths: [doomed.path], permanent: !result.trash }
				})
			);
			if (started?.data == null) return;
			shareJobs.track(started.data.jobId, 'delete', jobLabel('delete', doomed.name), ondone);
		} finally {
			close();
			busy = false;
		}
	}
</script>

<ConfirmDialog
	bind:open={
		() => checked !== null && target !== null,
		(open) => {
			if (!open) close();
		}
	}
	title={m.shares_delete_title({ name: checked?.target.name ?? '' })}
	{busy}
	onconfirm={confirm}
>
	{checked?.result.trash ? m.shares_delete_trash() : m.shares_delete_permanent()}
	{#snippet body()}
		{#if checked !== null && checked.result.listed.length > 0}
			<div class="flex flex-col gap-2 text-sm">
				<p>{m.shares_delete_listed_note()}</p>
				<ul class="list-disc pl-5">
					{#each checked.result.listed as listed (listed.contentId)}
						<li class="wrap-anywhere">{listed.title}</li>
					{/each}
				</ul>
			</div>
		{/if}
	{/snippet}
</ConfirmDialog>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
