<script lang="ts">
	import { refreshAll } from '$app/navigation';
	import * as m from '$lib/paraglide/messages.js';
	import * as Dialog from '$lib/components/ui/dialog';
	import RecoveryCodePanel from '$lib/components/recovery-code-panel.svelte';

	/**
	 * 作ったリカバリコードを見せる (→ docs/access.md「リカバリコード」)。`code` を入れると開き、
	 * 「保管しました」で閉じて `null` に戻す。
	 */
	let { code = $bindable(null), username }: { code?: string | null; username: string } = $props();

	async function handleConfirm() {
		code = null;
		// アカウントのページに、作った日を出し直す。
		await refreshAll();
	}
</script>

<!-- 開閉は `code` で決める。外を押しても Esc でも閉じない。うっかり閉じると二度と見られないため。 -->
<Dialog.Root open={code !== null}>
	<Dialog.Content
		interactOutsideBehavior="ignore"
		escapeKeydownBehavior="ignore"
		showCloseButton={false}
	>
		<Dialog.Header>
			<Dialog.Title>{m.recovery_code_dialog_title()}</Dialog.Title>
		</Dialog.Header>
		{#if code !== null}
			<Dialog.Body>
				<RecoveryCodePanel {code} {username} onconfirm={handleConfirm} />
			</Dialog.Body>
		{/if}
	</Dialog.Content>
</Dialog.Root>
