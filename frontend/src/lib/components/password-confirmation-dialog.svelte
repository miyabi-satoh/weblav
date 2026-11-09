<script lang="ts">
	import { dialogFormClass } from '$lib/dialog-form';
	import * as m from '$lib/paraglide/messages.js';
	import { client } from '$lib/api/client';
	import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
	import { passwordConfirmation } from '$lib/password-confirmation.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import PasswordInput from '$lib/components/password-input.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';

	/** 確かめ直しのダイアログ (→ `$lib/password-confirmation.svelte`)。ルートのレイアウトに1つだけ置く。 */
	let password = $state('');
	let confirming = $state(false);
	let error = $state<string | null>(null);

	$effect(() => {
		if (passwordConfirmation.open) {
			password = '';
			error = null;
		}
	});

	function cancel() {
		if (confirming) return;
		passwordConfirmation.settle(false);
	}

	async function handleSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (confirming) return;
		confirming = true;
		try {
			const { error: body, response } = await client.POST('/api/v1/auth/confirm-password', {
				body: { password }
			});
			if (!response.ok) {
				// 違ったときは閉じずに欄の下へ出し、打ち直せるようにする。
				error = errorMessage(body);
				return;
			}
			passwordConfirmation.settle(true);
		} catch {
			error = GENERIC_ERROR_MESSAGE();
		} finally {
			confirming = false;
		}
	}
</script>

<!-- 閉じる (外を押す・Esc・×) は「やめる」と同じ。開くのは middleware だけなので、setter は閉じるときだけ。 -->
<Dialog.Root
	bind:open={
		() => passwordConfirmation.open,
		(open) => {
			if (!open) cancel();
		}
	}
>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>{m.password_confirmation_dialog_title()}</Dialog.Title>
		</Dialog.Header>
		<form onsubmit={handleSubmit} class={dialogFormClass}>
			<Dialog.Body>
				<Field.Field data-invalid={error !== null}>
					<Field.FieldLabel for="password-confirmation-password"
						>{m.account_current_password_label()}</Field.FieldLabel
					>
					<PasswordInput
						id="password-confirmation-password"
						autocomplete="current-password"
						aria-invalid={error !== null}
						bind:value={password}
						required
					/>
					{#if error !== null}
						<Field.FieldError>{error}</Field.FieldError>
					{/if}
				</Field.Field>
			</Dialog.Body>
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={cancel}>{m.action_cancel()}</Button>
				<LoadingButton type="submit" loading={confirming}
					>{m.password_confirmation_submit()}</LoadingButton
				>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
