<script lang="ts">
	/**
	 * 保存していない変更があるまま別の画面へ移ろうとしたときの確認 (`LeaveGuard`)。
	 * 失うものが変更でない画面 (一度だけ見せるリカバリコード) は、題・本文・ボタンを差し替える。
	 */
	import type { LeaveGuard } from '#lib/leave-guard.svelte.ts';
	import * as m from '#lib/paraglide/messages.js';
	import * as AlertDialog from '#lib/components/ui/alert-dialog/index.ts';

	let {
		guard,
		ondiscard,
		title = m.common_leave_discard_title(),
		description = m.common_leave_discard_description(),
		action = m.action_discard_and_leave()
	}: {
		guard: LeaveGuard;
		/** 区画の変更を無かったことにする。 */
		ondiscard: () => void;
		title?: string;
		description?: string;
		action?: string;
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
			<AlertDialog.Title>{title}</AlertDialog.Title>
			<AlertDialog.Description>{description}</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>{m.action_cancel()}</AlertDialog.Cancel>
			<AlertDialog.Action variant="destructive" onclick={() => guard.discardAndLeave(ondiscard)}
				>{action}</AlertDialog.Action
			>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
