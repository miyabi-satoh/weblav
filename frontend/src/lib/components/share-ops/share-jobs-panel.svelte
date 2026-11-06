<script lang="ts">
	import { onMount } from 'svelte';
	import { refreshAll } from '$app/navigation';
	import { toast } from 'svelte-sonner';
	import * as m from '$lib/paraglide/messages.js';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import {
		jobErrorMessage,
		jobProgressRatio,
		jobProgressText,
		shareJobs
	} from '$lib/share-jobs.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import { Progress } from '$lib/components/ui/progress';

	/**
	 * 共有の中のコピー・移動・削除の進み具合 (→ docs/ui.md「共有の中の画面」)。
	 * ルートのレイアウトに置き、画面を移っても出し続ける。1秒たっても終わらないものだけを出す。
	 */
	let {
		loggedIn,
		bottomOffset
	}: {
		/** ログインしている間だけ、読み込み直す前から走っている処理を拾う。 */
		loggedIn: boolean;
		/** 画面の下に固定したプレイヤーの高さ。欄をその上に出す。 */
		bottomOffset: number;
	} = $props();

	const errorDialog = new ErrorDialogState();

	onMount(() => {
		shareJobs.onfinish = ({ job }) => {
			const error = jobErrorMessage(job);
			if (error !== undefined) {
				errorDialog.show(error, m.shares_job_failed_title());
				return;
			}
			// 飛ばしたものは置き先を見ても分からないので、件数を知らせる。
			if (job.state === 'succeeded' && job.skipped > 0) {
				toast.success(m.shares_job_skipped({ count: job.skipped }));
			}
		};
		return () => {
			shareJobs.onfinish = undefined;
		};
	});

	// ログインしたら拾う。拾ったものは誰が始めたか分からないので、終わったら今の画面を取り直す。
	$effect(() => {
		if (loggedIn) void shareJobs.resume(() => refreshAll());
	});
</script>

{#if shareJobs.visible.length > 0}
	<!-- 下の再生のバーに重ならないよう、その高さだけ上げる。 -->
	<section
		aria-label={m.shares_jobs_panel_label()}
		class="fixed inset-x-4 z-40 flex flex-col gap-3 rounded-lg border bg-background p-4 shadow-lg sm:right-6 sm:left-auto sm:w-96"
		style:bottom={`${bottomOffset + 16}px`}
	>
		{#each shareJobs.visible as tracked (tracked.job.id)}
			{@const progress = jobProgressText(tracked.job)}
			<div class="flex flex-col gap-2">
				<div class="flex items-start gap-3">
					<p class="min-w-0 flex-1 text-sm wrap-anywhere" role="status">{tracked.label}</p>
					<LoadingButton
						type="button"
						variant="outline"
						size="sm"
						loading={tracked.cancelling}
						onclick={() => shareJobs.cancel(tracked.job.id)}
					>
						{m.action_stop()}
					</LoadingButton>
				</div>
				<Progress
					class="h-1.5"
					aria-label={tracked.label}
					value={Math.round(jobProgressRatio(tracked.job) * 100)}
				/>
				{#if progress !== ''}
					<p class="text-xs text-muted-foreground">{progress}</p>
				{/if}
			</div>
		{/each}
	</section>
{/if}

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
