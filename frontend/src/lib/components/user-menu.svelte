<script lang="ts">
	import CircleUserRoundIcon from '@lucide/svelte/icons/circle-user-round';
	import { invalidateAll } from '$app/navigation';
	import { resolve } from '$app/paths';
	import * as m from '$lib/paraglide/messages.js';
	import { client } from '$lib/api/client';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { FormDirtyState } from '$lib/form-dirty.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import PasswordInput from '$lib/components/password-input.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import RecoveryCodeDialog from '$lib/components/recovery-code-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import { headerTextActionClass } from '$lib/header-action';
	import { cn } from '$lib/utils';
	import { toast } from 'svelte-sonner';

	let { username, onlogout }: { username: string; onlogout: () => void } = $props();

	let recoveryCodeOpen = $state(false);

	const errorDialog = new ErrorDialogState();

	// パスワード変更ダイアログ。
	let changePasswordOpen = $state(false);
	let currentPassword = $state('');
	let newPassword = $state('');
	let newPasswordConfirm = $state('');
	let changing = $state(false);
	let mismatch = $state(false);
	const changeForm = new FormDirtyState(() => ({
		currentPassword,
		newPassword,
		newPasswordConfirm
	}));

	function openChangePasswordDialog() {
		mismatch = false;
		currentPassword = '';
		newPassword = '';
		newPasswordConfirm = '';
		changeForm.markPristine();
		changePasswordOpen = true;
	}

	async function handleChangePasswordSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (changing) return;
		if (newPassword !== newPasswordConfirm) {
			mismatch = true;
			return;
		}
		mismatch = false;
		changing = true;
		const done = await errorDialog.attempt(() =>
			client.PUT('/api/v1/auth/password', {
				body: { currentPassword, newPassword }
			})
		);
		changing = false;
		if (done === null) return;
		changePasswordOpen = false;
		toast.success(m.change_password_done_toast());
	}

	// ユーザー名変更ダイアログ。少しだけ直すことが多いので、今の名前を入れた状態で開く。
	let changeUsernameOpen = $state(false);
	let newUsername = $state('');
	let usernamePassword = $state('');
	let renaming = $state(false);
	const renameForm = new FormDirtyState(() => ({
		newUsername,
		usernamePassword
	}));

	function openChangeUsernameDialog() {
		newUsername = username;
		usernamePassword = '';
		renameForm.markPristine();
		changeUsernameOpen = true;
	}

	async function handleChangeUsernameSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (renaming) return;
		renaming = true;
		const done = await errorDialog.attempt(() =>
			client.PUT('/api/v1/auth/username', {
				body: { currentPassword: usernamePassword, username: newUsername }
			})
		);
		renaming = false;
		if (done === null) return;
		changeUsernameOpen = false;
		toast.success(m.change_username_done_toast());
		// ヘッダーなどに出している自分の名前を引き直す。変更は済んでいるので、失敗を変更の失敗として見せない。
		await invalidateAll();
	}
</script>

<DropdownMenu.Root>
	<!-- 狭い幅でヘッダーからはみ出さないよう、Button の shrink-0 を打ち消し、残りの幅で名前を省略する。 -->
	<DropdownMenu.Trigger class={cn(headerTextActionClass, 'min-w-0 shrink')}>
		<!-- 隣の言語・テーマのアイコンと大きさを揃える (sm の既定は size-3.5)。 -->
		<CircleUserRoundIcon data-icon="inline-start" class="size-5" />
		<span class="max-w-40 min-w-0 truncate">{username}</span>
	</DropdownMenu.Trigger>
	<DropdownMenu.Content align="end">
		<DropdownMenu.Group>
			<DropdownMenu.Label class="wrap-anywhere">{username}</DropdownMenu.Label>
			<DropdownMenu.Separator />
			<!-- コンテンツ管理は `user` にも開放されている (→ docs/access.md「ロールと操作」)。 -->
			<DropdownMenu.Item>
				{#snippet child({ props })}
					<a href={resolve('/admin/contents')} {...props}>{m.admin_nav_label()}</a>
				{/snippet}
			</DropdownMenu.Item>
			<!-- ログイン後にマニュアルを引き直す導線がここにしかない (ログイン画面・
			     フォルダー選択・軸の画面のリンクは、いずれも使っている最中にしか出ない)。 -->
			<DropdownMenu.Item>
				{#snippet child({ props })}
					<a href={resolve('/help')} {...props}>{m.help_eyebrow()}</a>
				{/snippet}
			</DropdownMenu.Item>
			<DropdownMenu.Item onSelect={openChangeUsernameDialog}>
				{m.change_username_menu_item()}
			</DropdownMenu.Item>
			<DropdownMenu.Item onSelect={openChangePasswordDialog}>
				{m.change_password_menu_item()}
			</DropdownMenu.Item>
			<DropdownMenu.Item onSelect={() => (recoveryCodeOpen = true)}>
				{m.recovery_code_menu_item()}
			</DropdownMenu.Item>
			<DropdownMenu.Item onSelect={onlogout}>{m.logout_button()}</DropdownMenu.Item>
		</DropdownMenu.Group>
	</DropdownMenu.Content>
</DropdownMenu.Root>

<Dialog.Root bind:open={changePasswordOpen}>
	<Dialog.Content
		interactOutsideBehavior={changeForm.dirty ? 'ignore' : 'close'}
		escapeKeydownBehavior={changeForm.dirty ? 'ignore' : 'close'}
	>
		<Dialog.Header>
			<Dialog.Title>{m.change_password_dialog_title()}</Dialog.Title>
		</Dialog.Header>
		<form onsubmit={handleChangePasswordSubmit} class="flex flex-col gap-6">
			<Field.FieldGroup>
				<Field.Field>
					<Field.FieldLabel for="change-password-current" required
						>{m.account_current_password_label()}</Field.FieldLabel
					>
					<PasswordInput
						id="change-password-current"
						autocomplete="current-password"
						bind:value={currentPassword}
						required
					/>
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel for="change-password-new" required
						>{m.change_password_new_label()}</Field.FieldLabel
					>
					<PasswordInput
						id="change-password-new"
						autocomplete="new-password"
						bind:value={newPassword}
						required
					/>
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel for="change-password-new-confirm" required
						>{m.change_password_new_confirm_label()}</Field.FieldLabel
					>
					<PasswordInput
						id="change-password-new-confirm"
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
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (changePasswordOpen = false)}
					>{m.action_cancel()}</Button
				>
				<LoadingButton type="submit" loading={changing}>{m.action_save()}</LoadingButton>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<Dialog.Root bind:open={changeUsernameOpen}>
	<Dialog.Content
		interactOutsideBehavior={renameForm.dirty ? 'ignore' : 'close'}
		escapeKeydownBehavior={renameForm.dirty ? 'ignore' : 'close'}
	>
		<Dialog.Header>
			<Dialog.Title>{m.change_username_dialog_title()}</Dialog.Title>
		</Dialog.Header>
		<form onsubmit={handleChangeUsernameSubmit} class="flex flex-col gap-6">
			<Field.FieldGroup>
				<Field.Field>
					<Field.FieldLabel for="change-username-new" required
						>{m.change_username_label()}</Field.FieldLabel
					>
					<Input
						id="change-username-new"
						autocomplete="username"
						bind:value={newUsername}
						required
					/>
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel for="change-username-current-password" required
						>{m.account_current_password_label()}</Field.FieldLabel
					>
					<PasswordInput
						id="change-username-current-password"
						autocomplete="current-password"
						bind:value={usernamePassword}
						required
					/>
				</Field.Field>
			</Field.FieldGroup>
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (changeUsernameOpen = false)}
					>{m.action_cancel()}</Button
				>
				<LoadingButton type="submit" loading={renaming}>{m.action_save()}</LoadingButton>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<RecoveryCodeDialog bind:open={recoveryCodeOpen} {username} />

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
