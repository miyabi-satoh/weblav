<script lang="ts">
	import { untrack } from 'svelte';
	import { client } from '$lib/api/client';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { FormDirtyState } from '$lib/form-dirty.svelte';
	import { RecoveryCodeCreate } from '$lib/recovery-code-create.svelte';
	import { formatDate } from '$lib/format';
	import { SectionSaveState } from '$lib/section-save.svelte';
	import { masonry } from '$lib/masonry';
	import {
		adminPageClass,
		adminPageHeaderClass,
		pageHeadingTextClass,
		sectionCardClass,
		sectionGridClass
	} from '$lib/page-layout';
	import { pageTitle } from '$lib/page-title';
	import * as m from '$lib/paraglide/messages.js';
	import { Button } from '$lib/components/ui/button';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LeaveConfirmDialog from '$lib/components/leave-confirm-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import PasswordInput from '$lib/components/password-input.svelte';
	import RecoveryCodeDialog from '$lib/components/recovery-code-dialog.svelte';
	import type { PageProps } from './$types';

	/**
	 * 自分のアカウント (→ docs/ui.md「アカウントのページ」)。区画でじかに直し、確定したときに
	 * サーバーが求めればパスワードを聞く (→ `$lib/password-confirmation.svelte`)。
	 */
	type Part = 'username' | 'password';

	let { data }: PageProps = $props();
	let me = $derived(data.me);

	let username = $state(untrack(() => data.me.username));
	const usernameForm = new FormDirtyState(() => ({ username }));
	usernameForm.markPristine();

	let newPassword = $state('');
	let newPasswordConfirm = $state('');
	let mismatch = $state(false);
	const passwordForm = new FormDirtyState(() => ({ newPassword, newPasswordConfirm }));
	passwordForm.markPristine();

	const errorDialog = new ErrorDialogState();
	const sections = new SectionSaveState<Part>([usernameForm, passwordForm], errorDialog);

	async function handleUsernameSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (username.trim() === '') return;
		await sections.save('username', m.account_username_label(), async () => {
			const result = await errorDialog.attempt(() =>
				client.PUT('/api/v1/auth/username', { body: { username } })
			);
			if (result === null) return false;
			// サーバーが前後の空白を落とすので、そろえて保存したことにする。見出しの名前は取り直しで変わる。
			username = username.trim();
			usernameForm.markPristine();
			return true;
		});
	}

	async function handlePasswordSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (newPassword !== newPasswordConfirm) {
			mismatch = true;
			return;
		}
		mismatch = false;
		await sections.save('password', m.account_password_heading(), async () => {
			const result = await errorDialog.attempt(() =>
				client.PUT('/api/v1/auth/password', { body: { newPassword } })
			);
			if (result === null) return false;
			newPassword = '';
			newPasswordConfirm = '';
			passwordForm.markPristine();
			return true;
		});
	}

	// リカバリコード。作り直すと前のものは使えなくなるので、持っているときだけ先に確かめる。
	const recoveryCode = new RecoveryCodeCreate(errorDialog);
	let replaceConfirmOpen = $state(false);

	function createRecoveryCode() {
		if (recoveryCode.creating) return;
		replaceConfirmOpen = false;
		void recoveryCode.create();
	}

	function handleRecoveryCodeClick() {
		if (me.hasRecoveryCode) replaceConfirmOpen = true;
		else createRecoveryCode();
	}
</script>

<svelte:head><title>{pageTitle(m.account_title())}</title></svelte:head>

<div class={adminPageClass}>
	<div class={adminPageHeaderClass}>
		<h1 class={pageHeadingTextClass}>{m.account_title()}</h1>
	</div>
	<div class={sectionGridClass} {@attach masonry}>
		<form
			onsubmit={handleUsernameSubmit}
			aria-labelledby="account-username-heading"
			class={sectionCardClass}
		>
			<!-- 区画の欄はユーザー名だけなので、見出しを欄のラベルにして同じ語を2度出さない (ユーザーの編集ページと同じ)。 -->
			<h2 id="account-username-heading">
				<Field.FieldLabel for="account-username" required class="text-base font-semibold"
					>{m.account_username_label()}</Field.FieldLabel
				>
			</h2>
			<Input id="account-username" autocomplete="username" bind:value={username} required />
			<div>
				<LoadingButton
					type="submit"
					loading={sections.savingPart === 'username'}
					disabled={sections.saving || !usernameForm.dirty || username.trim() === ''}
					>{m.action_save()}</LoadingButton
				>
			</div>
		</form>

		<form
			onsubmit={handlePasswordSubmit}
			aria-labelledby="account-password-heading"
			class={sectionCardClass}
		>
			<h2 id="account-password-heading" class="text-base font-semibold">
				{m.account_password_heading()}
			</h2>
			<!-- パスワードマネージャーが新しいパスワードを結び付けられるよう、見えないユーザー名を添える。 -->
			<input
				type="text"
				class="sr-only"
				autocomplete="username"
				value={me.username}
				readonly
				tabindex="-1"
				aria-hidden="true"
			/>
			<Field.FieldGroup>
				<Field.Field>
					<Field.FieldLabel for="account-new-password" required
						>{m.common_new_password_label()}</Field.FieldLabel
					>
					<PasswordInput
						id="account-new-password"
						autocomplete="new-password"
						bind:value={newPassword}
						required
					/>
				</Field.Field>
				<Field.Field data-invalid={mismatch}>
					<Field.FieldLabel for="account-new-password-confirm" required
						>{m.common_new_password_confirm_label()}</Field.FieldLabel
					>
					<PasswordInput
						id="account-new-password-confirm"
						autocomplete="new-password"
						aria-invalid={mismatch}
						bind:value={newPasswordConfirm}
						required
					/>
					{#if mismatch}
						<Field.FieldError>{m.common_password_mismatch()}</Field.FieldError>
					{/if}
				</Field.Field>
			</Field.FieldGroup>
			<div>
				<LoadingButton
					type="submit"
					loading={sections.savingPart === 'password'}
					disabled={sections.saving || newPassword === '' || newPasswordConfirm === ''}
					>{m.action_save()}</LoadingButton
				>
			</div>
		</form>

		<section aria-labelledby="account-recovery-heading" class={sectionCardClass}>
			<h2 id="account-recovery-heading" class="text-base font-semibold">
				{m.recovery_code_label()}
			</h2>
			<p class="text-sm">
				{#if me.recoveryCodeCreatedAt}
					{m.account_recovery_code_created_on({ date: formatDate(me.recoveryCodeCreatedAt) })}
				{:else}
					{m.account_recovery_code_missing()}
				{/if}
			</p>
			<div>
				<LoadingButton
					variant="outline"
					loading={recoveryCode.creating}
					onclick={handleRecoveryCodeClick}
					>{me.hasRecoveryCode
						? m.account_recovery_code_replace_button()
						: m.account_recovery_code_create_button()}</LoadingButton
				>
			</div>
		</section>
	</div>
</div>

<AlertDialog.Root bind:open={replaceConfirmOpen}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{m.account_recovery_code_replace_title()}</AlertDialog.Title>
			<AlertDialog.Description
				>{m.account_recovery_code_replace_description()}</AlertDialog.Description
			>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>{m.action_cancel()}</AlertDialog.Cancel>
			<Button onclick={createRecoveryCode}>{m.account_recovery_code_replace_confirm()}</Button>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<RecoveryCodeDialog bind:code={recoveryCode.code} username={me.username} />

<LeaveConfirmDialog guard={sections.leaveGuard} ondiscard={() => sections.discard()} />

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
