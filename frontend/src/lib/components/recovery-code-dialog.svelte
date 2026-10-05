<script lang="ts">
	import { untrack } from 'svelte';
	import { invalidateAll } from '$app/navigation';
	import * as m from '$lib/paraglide/messages.js';
	import { client } from '$lib/api/client';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { FormDirtyState } from '$lib/form-dirty.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import PasswordInput from '$lib/components/password-input.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import RecoveryCodePanel from '$lib/components/recovery-code-panel.svelte';

	/** 自分のリカバリコードを作る・作り直す (→ docs/access.md「リカバリコード」)。今のパスワードを求める。 */
	let { open = $bindable(false), username }: { open?: boolean; username: string } = $props();

	const errorDialog = new ErrorDialogState();
	let currentPassword = $state('');
	let creating = $state(false);
	/** 作ったコード。`null` の間はパスワードの入力を見せる。 */
	let code = $state<string | null>(null);
	const passwordForm = new FormDirtyState(() => ({ currentPassword }));
	/** 外を押しても Esc でも閉じない間。パスワードの入力途中は、ほかのフォームのダイアログと同じく閉じない。 */
	let keepOpen = $derived(code !== null || passwordForm.dirty);

	$effect(() => {
		if (open) {
			currentPassword = '';
			code = null;
			// 入力のたびにこの effect が走り直さないよう、基準の読み取りは追わない。
			untrack(() => passwordForm.markPristine());
		}
	});

	async function handleSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (creating) return;
		creating = true;
		const done = await errorDialog.attempt(() =>
			client.POST('/api/v1/auth/recovery-code', { body: { currentPassword } })
		);
		creating = false;
		if (done === null) return;
		code = done.data?.recoveryCode ?? null;
	}

	async function handleConfirm() {
		open = false;
		// 管理画面の「まだ作っていない」の帯を消す。
		await invalidateAll();
	}
</script>

<Dialog.Root bind:open>
	<!-- コードを見せている間は、外を押しても Esc でも閉じない。うっかり閉じると二度と見られないため。 -->
	<Dialog.Content
		interactOutsideBehavior={keepOpen ? 'ignore' : 'close'}
		escapeKeydownBehavior={keepOpen ? 'ignore' : 'close'}
		showCloseButton={code === null}
	>
		<Dialog.Header>
			<Dialog.Title>{m.recovery_code_dialog_title()}</Dialog.Title>
			{#if code === null}
				<Dialog.Description>{m.recovery_code_dialog_description()}</Dialog.Description>
			{/if}
		</Dialog.Header>
		{#if code === null}
			<form onsubmit={handleSubmit} class="flex flex-col gap-6">
				<Field.Field>
					<Field.FieldLabel for="recovery-code-current-password" required
						>{m.recovery_code_dialog_current_password_label()}</Field.FieldLabel
					>
					<PasswordInput
						id="recovery-code-current-password"
						autocomplete="current-password"
						bind:value={currentPassword}
						required
					/>
				</Field.Field>
				<Dialog.Footer>
					<Button type="button" variant="outline" onclick={() => (open = false)}
						>{m.action_cancel()}</Button
					>
					<LoadingButton type="submit" loading={creating}>{m.recovery_code_create()}</LoadingButton>
				</Dialog.Footer>
			</form>
		{:else}
			<RecoveryCodePanel {code} {username} onconfirm={handleConfirm} />
		{/if}
	</Dialog.Content>
</Dialog.Root>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
