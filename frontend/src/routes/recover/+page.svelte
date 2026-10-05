<script lang="ts">
	import { goto, invalidateAll } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { client } from '$lib/api/client';
	import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import * as Card from '$lib/components/ui/card';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import PasswordInput from '$lib/components/password-input.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import RecoveryCodePanel from '$lib/components/recovery-code-panel.svelte';
	import { pageTitle } from '$lib/page-title';
	import {
		centeredPageClass,
		centeredPageInnerClass,
		centeredPageCardClass,
		centeredPageFooterLinkClass
	} from '$lib/page-layout';

	// リカバリコードで自分のパスワードを再設定する (→ docs/access.md「リカバリコード」)。
	let username = $state('');
	let recoveryCode = $state('');
	let password = $state('');
	let passwordConfirm = $state('');
	let loading = $state(false);
	/** 再設定できたら、使い切ったコードの代わりに作った新しいコードを見せる。 */
	let newCode = $state<string | null>(null);
	/** 控えに書くユーザー名。入力の大文字・小文字でなく、アカウントの表記に合わせる。 */
	let accountName = $state('');
	const errorDialog = new ErrorDialogState();
	const passwordMismatch = $derived(passwordConfirm !== '' && password !== passwordConfirm);

	async function handleSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (loading || passwordMismatch) return;
		loading = true;
		try {
			const { data, error, response } = await client.POST('/api/v1/auth/recover', {
				body: { username, recoveryCode, newPassword: password }
			});
			if (!response.ok || !data) {
				errorDialog.show(errorMessage(error));
				return;
			}
			accountName = data.user.username;
			newCode = data.recoveryCode;
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			loading = false;
		}
	}

	// 再設定と同時にログイン済みになっている。ヘッダーの表示を取り直してから移る。
	async function handleKept() {
		await invalidateAll();
		await goto(resolve('/admin/contents'));
	}
</script>

<svelte:head><title>{pageTitle(m.recover_title())}</title></svelte:head>

<div class={centeredPageClass}>
	<div class={centeredPageInnerClass}>
		<Card.Root class={centeredPageCardClass}>
			<Card.Header class="text-center">
				<Card.Title class="text-xl">{m.recover_title()}</Card.Title>
				<Card.Description>
					{newCode === null ? m.recover_description() : m.recover_done_description()}
				</Card.Description>
			</Card.Header>
			<Card.Content>
				{#if newCode !== null}
					<RecoveryCodePanel code={newCode} username={accountName} onconfirm={handleKept} />
				{:else}
					<form onsubmit={handleSubmit}>
						<Field.FieldGroup>
							<Field.Field>
								<Field.FieldLabel for="username" required
									>{m.recover_username_label()}</Field.FieldLabel
								>
								<Input id="username" autocomplete="username" bind:value={username} required />
							</Field.Field>
							<Field.Field>
								<Field.FieldLabel for="recovery-code" required
									>{m.recovery_code_label()}</Field.FieldLabel
								>
								<Input
									id="recovery-code"
									autocomplete="off"
									autocapitalize="characters"
									spellcheck="false"
									bind:value={recoveryCode}
									required
								/>
							</Field.Field>
							<Field.Field>
								<Field.FieldLabel for="new-password" required
									>{m.recover_new_password_label()}</Field.FieldLabel
								>
								<PasswordInput
									id="new-password"
									autocomplete="new-password"
									bind:value={password}
									required
								/>
							</Field.Field>
							<Field.Field>
								<Field.FieldLabel for="new-password-confirm" required
									>{m.recover_new_password_confirm_label()}</Field.FieldLabel
								>
								<PasswordInput
									id="new-password-confirm"
									autocomplete="new-password"
									aria-invalid={passwordMismatch}
									bind:value={passwordConfirm}
									required
								/>
								{#if passwordMismatch}
									<Field.FieldError>{m.common_password_mismatch()}</Field.FieldError>
								{/if}
							</Field.Field>
							<Field.Field>
								<LoadingButton type="submit" class="w-full" {loading} disabled={passwordMismatch}>
									{m.recover_submit()}
								</LoadingButton>
							</Field.Field>
						</Field.FieldGroup>
					</form>
				{/if}
			</Card.Content>
			{#if newCode === null}
				<Card.Footer class="justify-center">
					<a href={resolve('/login')} class={centeredPageFooterLinkClass}>
						{m.recover_back_to_login()}
					</a>
				</Card.Footer>
			{/if}
		</Card.Root>
	</div>

	<ErrorDialog
		bind:open={errorDialog.open}
		message={errorDialog.message}
		title={m.recover_error_title()}
	/>
</div>
