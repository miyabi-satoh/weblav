<script lang="ts">
	import {
		adminPageClass,
		adminPageHeaderClass,
		pageHeadingTextClass,
		adminTableBandClass
	} from '$lib/page-layout';
	import { goto, refreshAll } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { dialogFormClass } from '$lib/dialog-form';
	import { client } from '$lib/api/client';
	import { page } from '$app/state';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { formatDate } from '$lib/format';
	import * as m from '$lib/paraglide/messages.js';
	import type { components } from '$lib/api/schema';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import PasswordInput from '$lib/components/password-input.svelte';
	import * as Table from '$lib/components/ui/table';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import RowActionsMenu from '$lib/components/row-actions-menu.svelte';
	import UserDeleteDialog from '$lib/components/user-delete-dialog.svelte';
	import type { PageProps } from './$types';
	import { pageTitle } from '$lib/page-title';
	import { roleLabel } from '$lib/role-labels';
	import {
		actionColumnClass,
		columnFromMdClass,
		fitColumnClass,
		rowLinkClass,
		truncatingColumnClass
	} from '$lib/table-columns';
	import { FormDirtyState } from '$lib/form-dirty.svelte';

	type UserListItem = components['schemas']['UserListItem'];

	let { data }: PageProps = $props();
	let users = $derived(data.users);
	// 自分の行は押せなくする。自分はユーザー管理でなく、ユーザーメニューの「アカウント」から直す
	// (→ docs/ui.md「管理画面のユーザーの一覧」)。
	let selfId = $derived(page.data.user?.id);

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
		if (!created?.data) return;
		createOpen = false;
		// 共有・コンテンツの追加と同じく、作ったユーザーの編集ページへ移り、続けて権限を決められるようにする。
		await goto(resolve('/admin/users/[id]', { id: String(created.data.id) }));
	}

	let deletingUser = $state<UserListItem | null>(null);
</script>

<svelte:head><title>{pageTitle(m.admin_users_nav_title())}</title></svelte:head>

<div class={adminPageClass}>
	<div class={adminPageHeaderClass}>
		<h1 class={pageHeadingTextClass}>{m.admin_users_nav_title()}</h1>
		<Button onclick={openCreateDialog}>{m.admin_users_add_button()}</Button>
	</div>

	<div class={adminTableBandClass}>
		<Table.Root>
			<Table.Header>
				<Table.Row>
					<Table.Head>{m.admin_users_table_username()}</Table.Head>
					<Table.Head class={fitColumnClass}>{m.admin_users_role_label()}</Table.Head>
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
					<!-- 行のどこを押しても編集ページへ移る (コンテンツ管理の表と同じ)。押せる範囲はユーザー名のリンクの
					     ::after で行いっぱいに広げ、⋮は DOM で後ろに置いてその上に重ねる (→ docs/ui.md「UI 全般」)。
					     FIX: 古い iOS の WebKit は tr の relative を ::after の基準にしない (WebKit Bug 240961)。
					     transform なら基準にするので併せて付ける。 -->
					{@const isSelf = target.id === selfId}
					<!-- 自分の行は押しても何も起きないので、指を重ねても塗らない (押せそうに見せない)。 -->
					<Table.Row class={['relative transform-gpu', isSelf && 'hover:bg-transparent']}>
						<Table.Cell class={truncatingColumnClass}>
							{#if isSelf}
								<span class="block truncate" data-testid="user-username"
									>{m.admin_users_self_name({ username: target.username })}</span
								>
							{:else}
								<a
									href={resolve('/admin/users/[id]', { id: String(target.id) })}
									class={[rowLinkClass, 'block after:left-0']}
									data-testid="user-username">{target.username}</a
								>
							{/if}
						</Table.Cell>
						<!-- 権限は編集ページで変える。 -->
						<Table.Cell class={fitColumnClass}>{roleLabel(target.role)}</Table.Cell>
						<Table.Cell class={[columnFromMdClass, fitColumnClass, 'text-muted-foreground']}>
							{formatDate(target.createdAt)}
						</Table.Cell>
						<Table.Cell class={actionColumnClass}>
							{#if !isSelf}
								<RowActionsMenu name={target.username}>
									<!-- ユーザー名・権限・パスワードの再設定は、どれも編集ページで行う。 -->
									<DropdownMenu.Item>
										{#snippet child({ props })}
											<a href={resolve('/admin/users/[id]', { id: String(target.id) })} {...props}
												>{m.action_edit_page()}</a
											>
										{/snippet}
									</DropdownMenu.Item>
									<DropdownMenu.Item variant="destructive" onSelect={() => (deletingUser = target)}>
										{m.action_delete()}
									</DropdownMenu.Item>
								</RowActionsMenu>
							{/if}
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
		<form onsubmit={handleCreateSubmit} class={dialogFormClass}>
			<Dialog.Body>
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
					<Field.Field data-invalid={createPasswordMismatch}>
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
			</Dialog.Body>
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (createOpen = false)}
					>{m.action_cancel()}</Button
				>
				<LoadingButton type="submit" loading={creating}>{m.action_add()}</LoadingButton>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<UserDeleteDialog bind:user={deletingUser} ondeleted={() => refreshAll()} />

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
