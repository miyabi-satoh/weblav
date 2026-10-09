<script lang="ts">
	import {
		adminPageClass,
		adminPageHeaderClass,
		pageHeadingTextClass,
		adminTableBandClass
	} from '$lib/page-layout';
	import { invalidateAll } from '$app/navigation';
	import { page } from '$app/state';
	import { client } from '$lib/api/client';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { formatDate, formatNumber } from '$lib/format';
	import * as m from '$lib/paraglide/messages.js';
	import type { components } from '$lib/api/schema';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import PasswordInput from '$lib/components/password-input.svelte';
	import * as Select from '$lib/components/ui/select';
	import * as Table from '$lib/components/ui/table';
	import ConfirmDialog from '$lib/components/confirm-dialog.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import RowActionsMenu from '$lib/components/row-actions-menu.svelte';
	import { toast } from 'svelte-sonner';
	import type { PageProps } from './$types';
	import { pageTitle } from '$lib/page-title';
	import { roleLabel } from '$lib/role-labels';
	import {
		actionColumnClass,
		columnFromMdClass,
		fitColumnClass,
		truncatingColumnClass
	} from '$lib/table-columns';
	import { FormDirtyState } from '$lib/form-dirty.svelte';

	type UserListItem = components['schemas']['UserListItem'];
	type Role = components['schemas']['Role'];

	let { data }: PageProps = $props();
	// 権限変更・削除のあと、サーバーの応答をローカルへ反映できるよう $derived を使う
	// (再代入のみで更新し、in-place mutate しない)。
	let users = $derived(data.users);

	// 最後の1人の admin は降格・削除の操作を無効にする (→ docs/access.md「admin の最後の1人」)。
	let adminCount = $derived(users.filter((u) => u.role === 'admin').length);
	function isSoleAdmin(target: UserListItem): boolean {
		return target.role === 'admin' && adminCount === 1;
	}

	let currentUserId = $derived(page.data.user?.id);

	const errorDialog = new ErrorDialogState();

	// 作成ダイアログ。
	let createOpen = $state(false);
	let createUsername = $state('');
	let createPassword = $state('');
	let createPasswordConfirm = $state('');
	let creating = $state(false);
	// パスワードの不一致はまだ何も処理していないので、エラーダイアログ
	// (「処理に失敗しました」) ではなく確認欄の下に出す。
	let createPasswordMismatch = $state(false);
	// 入力中のダイアログを外側クリックやEscで閉じてしまわないようにする
	// (コンテンツ管理のフォームと同じ扱い)。
	const createForm = new FormDirtyState(() => ({
		username: createUsername,
		password: createPassword,
		passwordConfirm: createPasswordConfirm
	}));

	function openCreateDialog() {
		createPasswordMismatch = false;
		createUsername = '';
		createPassword = '';
		createPasswordConfirm = '';
		createForm.markPristine();
		createOpen = true;
	}

	async function handleCreateSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (creating) return;
		// サーバー側にはCLIの`--create-user`のような確認入力の仕組みが無い(1回きりの
		// リクエストのため)。ここで一致を確かめてから送る。
		if (createPassword !== createPasswordConfirm) {
			createPasswordMismatch = true;
			return;
		}
		createPasswordMismatch = false;
		creating = true;
		const created = await errorDialog.attempt(() =>
			client.POST('/api/v1/admin/users', {
				body: { username: createUsername, password: createPassword }
			})
		);
		creating = false;
		if (created === null) return;
		createOpen = false;
		// 並びはサーバーの `ORDER BY username COLLATE NOCASE` に従う。手元で並べ替えると
		// 畳み方が違い、作った直後だけ位置がずれる (→ src/api/users.rs)。
		// 作成は成功しているので、再検証の失敗を作成の失敗として見せない。
		await invalidateAll();
	}

	// 権限変更。行ごとのSelectから直接送信する(確認ダイアログは挟まない。
	// 誤操作しても再度切り替えるだけで戻せるため)。
	let changingRoleId = $state<number | null>(null);
	async function changeRole(target: UserListItem, role: Role) {
		if (role === target.role || changingRoleId !== null) return;
		changingRoleId = target.id;
		try {
			await errorDialog.attempt(
				() =>
					client.PUT('/api/v1/admin/users/{id}/role', {
						params: { path: { id: target.id } },
						body: { role }
					}),
				async ({ data: updated }) => {
					// 2xx なのに本文が無いのは想定外。汎用のエラーとして出す。
					if (!updated) throw new Error('empty response');
					users = users.map((u) => (u.id === updated.id ? updated : u));
					// 自分自身をadminから降格した場合、この画面はもう開けなくなる
					// (→ +layout.tsのガード)。再検証してリダイレクトさせる。
					if (target.id === currentUserId && role !== 'admin') {
						await invalidateAll();
					}
				}
			);
		} finally {
			changingRoleId = null;
		}
	}

	// パスワード再設定ダイアログ。
	let resettingUser = $state<UserListItem | null>(null);
	let resetPassword = $state('');
	let resetPasswordConfirm = $state('');
	let resetting = $state(false);
	let resetPasswordMismatch = $state(false);
	const resetForm = new FormDirtyState(() => ({
		password: resetPassword,
		passwordConfirm: resetPasswordConfirm
	}));

	function openResetDialog(target: UserListItem) {
		resetPasswordMismatch = false;
		resetPassword = '';
		resetPasswordConfirm = '';
		resetForm.markPristine();
		resettingUser = target;
	}

	async function handleResetSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (resetting || resettingUser === null) return;
		if (resetPassword !== resetPasswordConfirm) {
			resetPasswordMismatch = true;
			return;
		}
		resetPasswordMismatch = false;
		resetting = true;
		const target = resettingUser;
		const done = await errorDialog.attempt(() =>
			client.PUT('/api/v1/admin/users/{id}/password', {
				params: { path: { id: target.id } },
				body: { password: resetPassword }
			})
		);
		resetting = false;
		if (done === null) return;
		resettingUser = null;
		toast.success(m.admin_users_reset_password_done_toast({ username: target.username }));
	}

	// ユーザー名変更ダイアログ。少しだけ直すことが多いので、今の名前を入れた状態で開く。
	let renamingUser = $state<UserListItem | null>(null);
	let renameUsername = $state('');
	let renaming = $state(false);
	const renameForm = new FormDirtyState(() => ({ username: renameUsername }));

	function openRenameDialog(target: UserListItem) {
		renameUsername = target.username;
		renameForm.markPristine();
		renamingUser = target;
	}

	async function handleRenameSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (renaming || renamingUser === null) return;
		renaming = true;
		const target = renamingUser;
		const done = await errorDialog.attempt(() =>
			client.PUT('/api/v1/admin/users/{id}/username', {
				params: { path: { id: target.id } },
				body: { username: renameUsername }
			})
		);
		renaming = false;
		if (done === null) return;
		renamingUser = null;
		// 並びはサーバーの `ORDER BY username COLLATE NOCASE` に従う (作成と同じ理由)。
		// 自分の名前を変えたときのヘッダーの名前も、ここで引き直す。
		await invalidateAll();
	}

	// 削除確認ダイアログ。
	let deletingUser = $state<UserListItem | null>(null);
	let deleting = $state(false);

	async function handleDelete() {
		if (deleting || deletingUser === null) return;
		deleting = true;
		const target = deletingUser;
		try {
			await errorDialog.attempt(
				() =>
					client.DELETE('/api/v1/admin/users/{id}', {
						params: { path: { id: target.id } }
					}),
				async () => {
					deletingUser = null;
					users = users.filter((u) => u.id !== target.id);
					// 自分自身を削除した場合、次のリクエストで401になる(→ src/auth.rsのAuthUser)。
					// 再検証してログイン画面へのリダイレクトを起こさせる。
					if (target.id === currentUserId) {
						await invalidateAll();
					}
				}
			);
		} finally {
			deleting = false;
		}
	}
</script>

<svelte:head><title>{pageTitle(m.admin_users_title())}</title></svelte:head>

<div class={adminPageClass}>
	<div class={adminPageHeaderClass}>
		<h1 class={pageHeadingTextClass}>{m.admin_users_title()}</h1>
		<Button onclick={openCreateDialog}>{m.admin_users_add_button()}</Button>
	</div>

	<div class={adminTableBandClass}>
		<Table.Root>
			<Table.Header>
				<Table.Row>
					<Table.Head>{m.admin_users_table_username()}</Table.Head>
					<Table.Head class={fitColumnClass}>{m.admin_users_table_role()}</Table.Head>
					<Table.Head class={[columnFromMdClass, fitColumnClass]}
						>{m.admin_users_table_created_at()}</Table.Head
					>
					<!-- 見出しは読み上げにだけ残す (コンテンツ管理と同じ。→ docs/ui.md「UI 全般」)。 -->
					<Table.Head class={actionColumnClass}>
						<span class="sr-only">{m.common_table_actions()}</span>
					</Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each users as target (target.id)}
					<Table.Row>
						<Table.Cell class={truncatingColumnClass} data-testid="user-username"
							>{target.username}</Table.Cell
						>
						<Table.Cell class={fitColumnClass}>
							<Select.Root
								type="single"
								bind:value={() => target.role, (role) => changeRole(target, role as Role)}
								disabled={changingRoleId === target.id || isSoleAdmin(target)}
							>
								<!-- 表の中なので、見た目は小さいまま押せる範囲だけを 44px に広げる sm を使う
							     (→ select-trigger.svelte・docs/ui.md「UI 全般」)。 -->
								<Select.Trigger
									size="sm"
									class="w-32"
									aria-label={m.admin_users_role_row_label({
										username: target.username,
										role: roleLabel(target.role)
									})}
								>
									{roleLabel(target.role)}
								</Select.Trigger>
								<Select.Content>
									<Select.Item value="admin">{m.admin_users_role_admin()}</Select.Item>
									<Select.Item value="user">{m.admin_users_role_user()}</Select.Item>
								</Select.Content>
							</Select.Root>
							{#if isSoleAdmin(target)}
								<!-- 列は中身の幅まで縮めるので、注記は選択欄の幅で折り返す。 -->
								<p class="mt-1 w-32 text-xs whitespace-normal text-muted-foreground">
									{m.admin_users_last_admin_note()}
								</p>
							{/if}
						</Table.Cell>
						<Table.Cell class={[columnFromMdClass, fitColumnClass, 'text-muted-foreground']}>
							{formatDate(target.createdAt)}
						</Table.Cell>
						<Table.Cell class={actionColumnClass}>
							<RowActionsMenu name={target.username}>
								<DropdownMenu.Item onSelect={() => openRenameDialog(target)}>
									{m.admin_users_rename_button()}
								</DropdownMenu.Item>
								<DropdownMenu.Item onSelect={() => openResetDialog(target)}>
									{m.admin_users_reset_password_button()}
								</DropdownMenu.Item>
								<!-- 最後の管理者は消せない (→ docs/access.md「ユーザーの削除と作成者」)。 -->
								<DropdownMenu.Item
									variant="destructive"
									disabled={isSoleAdmin(target)}
									onSelect={() => (deletingUser = target)}
								>
									{m.action_delete()}
								</DropdownMenu.Item>
							</RowActionsMenu>
						</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	</div>
</div>

<Dialog.Root bind:open={createOpen}>
	<Dialog.Content
		interactOutsideBehavior={createForm.dirty ? 'ignore' : 'close'}
		escapeKeydownBehavior={createForm.dirty ? 'ignore' : 'close'}
	>
		<Dialog.Header>
			<Dialog.Title>{m.admin_users_dialog_create_title()}</Dialog.Title>
		</Dialog.Header>
		<form onsubmit={handleCreateSubmit} class="flex flex-col gap-6">
			<Field.FieldGroup>
				<Field.Field>
					<Field.FieldLabel for="user-username" required
						>{m.admin_users_dialog_username_label()}</Field.FieldLabel
					>
					<!-- ブラウザーのパスワードマネージャーが管理者自身の資格情報を入れてしまう
					     (入れたまま保存すると、新しいユーザーに管理者と同じパスワードが付く)。
					     ログインフォームと判定させないため、自動入力を切る。 -->
					<Input id="user-username" autocomplete="off" bind:value={createUsername} required />
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel for="user-password" required
						>{m.admin_users_dialog_password_label()}</Field.FieldLabel
					>
					<PasswordInput
						id="user-password"
						autocomplete="new-password"
						bind:value={createPassword}
						required
					/>
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel for="user-password-confirm" required
						>{m.admin_users_dialog_password_confirm_label()}</Field.FieldLabel
					>
					<PasswordInput
						id="user-password-confirm"
						autocomplete="new-password"
						aria-invalid={createPasswordMismatch}
						bind:value={createPasswordConfirm}
						required
					/>
					{#if createPasswordMismatch}
						<Field.FieldError>{m.common_password_mismatch()}</Field.FieldError>
					{/if}
				</Field.Field>
			</Field.FieldGroup>
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (createOpen = false)}
					>{m.action_cancel()}</Button
				>
				<LoadingButton type="submit" loading={creating}>{m.action_save()}</LoadingButton>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<Dialog.Root
	bind:open={
		() => resettingUser !== null,
		(open) => {
			if (!open) resettingUser = null;
		}
	}
>
	<Dialog.Content
		interactOutsideBehavior={resetForm.dirty ? 'ignore' : 'close'}
		escapeKeydownBehavior={resetForm.dirty ? 'ignore' : 'close'}
	>
		<Dialog.Header>
			<Dialog.Title>{m.admin_users_reset_password_dialog_title()}</Dialog.Title>
			<Dialog.Description>
				{m.admin_users_reset_password_dialog_description({
					username: resettingUser?.username ?? ''
				})}
			</Dialog.Description>
		</Dialog.Header>
		<form onsubmit={handleResetSubmit} class="flex flex-col gap-6">
			<Field.FieldGroup>
				<Field.Field>
					<Field.FieldLabel for="reset-password" required
						>{m.admin_users_dialog_password_label()}</Field.FieldLabel
					>
					<!-- 再設定はユーザー名の欄が無く、ブラウザーにログインフォームと判定されない。
					     `new-password` にすると Chrome のパスワード生成が2つの欄に干渉し、
					     確認欄が空のまま残ることがあるため、こちらは `off` にする。 -->
					<PasswordInput
						id="reset-password"
						autocomplete="off"
						bind:value={resetPassword}
						required
					/>
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel for="reset-password-confirm" required
						>{m.admin_users_dialog_password_confirm_label()}</Field.FieldLabel
					>
					<PasswordInput
						id="reset-password-confirm"
						autocomplete="off"
						aria-invalid={resetPasswordMismatch}
						bind:value={resetPasswordConfirm}
						required
					/>
					{#if resetPasswordMismatch}
						<Field.FieldError>{m.common_password_mismatch()}</Field.FieldError>
					{/if}
				</Field.Field>
			</Field.FieldGroup>
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (resettingUser = null)}
					>{m.action_cancel()}</Button
				>
				<LoadingButton type="submit" loading={resetting}>{m.action_save()}</LoadingButton>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<Dialog.Root
	bind:open={
		() => renamingUser !== null,
		(open) => {
			if (!open) renamingUser = null;
		}
	}
>
	<Dialog.Content
		interactOutsideBehavior={renameForm.dirty ? 'ignore' : 'close'}
		escapeKeydownBehavior={renameForm.dirty ? 'ignore' : 'close'}
	>
		<Dialog.Header>
			<Dialog.Title>{m.admin_users_rename_dialog_title()}</Dialog.Title>
			<Dialog.Description>
				{m.admin_users_rename_dialog_description({ username: renamingUser?.username ?? '' })}
			</Dialog.Description>
		</Dialog.Header>
		<form onsubmit={handleRenameSubmit} class="flex flex-col gap-6">
			<Field.FieldGroup>
				<Field.Field>
					<Field.FieldLabel for="rename-username" required
						>{m.admin_users_dialog_username_label()}</Field.FieldLabel
					>
					<!-- 作成と同じく、管理者自身の資格情報を自動入力させない。 -->
					<Input id="rename-username" autocomplete="off" bind:value={renameUsername} required />
				</Field.Field>
			</Field.FieldGroup>
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (renamingUser = null)}
					>{m.action_cancel()}</Button
				>
				<LoadingButton type="submit" loading={renaming}>{m.action_save()}</LoadingButton>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<ConfirmDialog
	bind:open={
		() => deletingUser !== null,
		(open) => {
			if (!open) deletingUser = null;
		}
	}
	title={m.admin_users_delete_confirm_title()}
	busy={deleting}
	onconfirm={handleDelete}
>
	{m.admin_users_delete_confirm_description({ username: deletingUser?.username ?? '' })}
	{#if deletingUser !== null && deletingUser.privateContentCount > 0}
		<br />
		{m.admin_users_delete_confirm_private_note({
			count: formatNumber(deletingUser.privateContentCount)
		})}
	{/if}
</ConfirmDialog>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
