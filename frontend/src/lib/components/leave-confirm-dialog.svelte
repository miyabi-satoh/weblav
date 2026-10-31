<script lang="ts">
	/**
	 * 保存していない変更があるまま別の画面へ移ろうとしたときの確認 (`LeaveGuard`)。
	 */
	import type { LeaveGuard } from '$lib/leave-guard.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';

	let {
		guard,
		ondiscard
	}: {
		guard: LeaveGuard;
		/** 区画の変更を無かったことにする。 */
		ondiscard: () => void;
	} = $props();
</script>

<AlertDialog.Root
	bind:open={
		() => guard.pending !== null,
		(open) => {
			if (!open) guard.pending = null;
		}
	}
>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{m.common_leave_discard_title()}</AlertDialog.Title>
			<AlertDialog.Description>{m.common_leave_discard_description()}</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>{m.action_cancel()}</AlertDialog.Cancel>
			<AlertDialog.Action variant="destructive" onclick={() => guard.discardAndLeave(ondiscard)}
				>{m.action_discard_and_leave()}</AlertDialog.Action
			>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
