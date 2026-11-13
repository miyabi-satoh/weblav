<script lang="ts">
	import { dialogFormClass } from '#lib/dialog-form.ts';
	import * as m from '#lib/paraglide/messages.js';
	import { client } from '#lib/api/client.ts';
	import { GENERIC_ERROR_MESSAGE, errorCode, errorMessage } from '#lib/api/errors.ts';
	import { passwordConfirmation } from '#lib/password-confirmation.svelte.ts';
	import { Button } from '#lib/components/ui/button/index.ts';
	import * as Dialog from '#lib/components/ui/dialog/index.ts';
	import * as Field from '#lib/components/ui/field/index.ts';
	import PasswordInput from '#lib/components/password-input.svelte';
	import LoadingButton from '#lib/components/loading-button.svelte';

	/** 確かめ直しのダイアログ (→ `#lib/password-confirmation.svelte.ts`)。ルートのレイアウトに1つだけ置く。 */
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
				// ログインが切れていたら (別のタブでログアウトしたなど)、`client` がログインへ送るので、
				// 閉じて元の要求もやめたことにする。開いたままだとログインの画面に重なって残る。
				if (errorCode(body) === 'unauthorized') {
					passwordConfirmation.settle(false);
					return;
				}
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
