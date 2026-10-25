<script lang="ts">
	import LoadingButton from '$lib/components/loading-button.svelte';
	import * as m from '$lib/paraglide/messages.js';

	type Props = {
		/** 進み具合の文言 (`$lib/archive-scan.ts`)。 */
		message: string;
		/** 中止を頼む。止まると、スキャンを始めた側に `cancelled` の結果が返る。 */
		oncancel: () => Promise<void>;
	};

	let { message, oncancel }: Props = $props();

	// 頼んでから止まるまでの間に押し直させない。止まればこの表示ごと消える。
	let cancelling = $state(false);

	async function cancel() {
		cancelling = true;
		try {
			await oncancel();
		} catch {
			// 頼めなければスキャンは続くので、押し直せるように戻す。
			cancelling = false;
		}
	}
</script>

<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
	<p role="status" class="text-sm text-muted-foreground">{message}</p>
	<LoadingButton type="button" variant="outline" size="sm" loading={cancelling} onclick={cancel}>
		{m.archive_scan_cancel_button()}
	</LoadingButton>
</div>
