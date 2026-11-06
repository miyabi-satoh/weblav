<script lang="ts">
	import { untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { resolve } from '$app/paths';
	import { client } from '$lib/api/client';
	import { isSoleAdmin } from '$lib/auth';
	import { dialogFormClass } from '$lib/dialog-form';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { FormDirtyState } from '$lib/form-dirty.svelte';
	import { formatDate } from '$lib/format';
	import { gotoWithoutLeaveConfirm } from '$lib/leave-confirm';
	import { SectionSaveState } from '$lib/section-save.svelte';
	import { masonry } from '$lib/masonry';
	import {
		adminPageClass,
		pageHeadingInRowClass,
		sectionCardClass,
		sectionGridClass
	} from '$lib/page-layout';
	import { pageTitle } from '$lib/page-title';
	import * as m from '$lib/paraglide/messages.js';
	import { roleLabel } from '$lib/role-labels';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import * as RadioGroup from '$lib/components/ui/radio-group';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LeaveConfirmDialog from '$lib/components/leave-confirm-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import PasswordInput from '$lib/components/password-input.svelte';
	import DeleteSection from '$lib/components/delete-section.svelte';
	import UserDeleteDialog from '$lib/components/user-delete-dialog.svelte';
	import BackLink from '$lib/components/back-link.svelte';
	import type { components } from '$lib/api/schema';
	import type { PageProps } from './$types';

	type UserListItem = components['schemas']['UserListItem'];
	type Role = components['schemas']['Role'];
	/** 保存ボタンを持つ区画。 */
	type Part = 'username' | 'role';

	const ROLES: Role[] = ['admin', 'user'];

	let { data }: PageProps = $props();

	let target = $derived(data.target);
	// 最後の管理者は降格も削除もできない (→ docs/access.md「admin の最後の1人」)。サーバーも同じ決まりで断る。
	let soleAdmin = $derived(isSoleAdmin(data.target, data.users));

	// 区画ごとに保存する (→ docs/ui.md「編集ページの区画」)。保存した後は応答の値で入れ直すので、読み込みは初回だけでよい。
	let username = $state(untrack(() => data.target.username));
	const usernameForm = new FormDirtyState(() => ({ username }));
	usernameForm.markPristine();

	let role = $state<Role>(untrack(() => data.target.role));
	const roleForm = new FormDirtyState(() => ({ role }));
	roleForm.markPristine();

	let deletingUser = $state<UserListItem | null>(null);
	const errorDialog = new ErrorDialogState();

	// 保存の後に見出しを取り直すので、自分の名前を変えたときのヘッダーの名前も引き直される。
	// 自分を一般ユーザーにしたときは、この画面をもう開けないので、`admin/+layout.ts` のガードがホームへ移す。
	const sections = new SectionSaveState<Part>([usernameForm, roleForm], errorDialog);

	async function handleUsernameSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (sections.saving || username.trim() === '') return;
		await sections.save('username', m.admin_users_dialog_username_label(), async () => {
			// 失敗したら値を残す。名前が重なったときに、入れ直せるようにする。
			const result = await errorDialog.attempt(() =>
				client.PUT('/api/v1/admin/users/{id}/username', {
					params: { path: { id: target.id } },
					body: { username }
				})
			);
			if (result?.data) {
				// サーバーが前後の空白を落とすので、保存された値で入れ直す。
				username = result.data.username;
				usernameForm.markPristine();
				return true;
			}
			return false;
		});
	}

	async function handleRoleSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (sections.saving) return;
		await sections.save('role', m.admin_users_role_label(), async () => {
			const result = await errorDialog.attempt(() =>
				client.PUT('/api/v1/admin/users/{id}/role', {
					params: { path: { id: target.id } },
					body: { role }
				})
			);
			if (result?.data) {
				role = result.data.role;
				roleForm.markPristine();
				return true;
			}
			return false;
		});
	}

	// パスワードの再設定ダイアログ。
	let resetOpen = $state(false);
	let resetPassword = $state('');
	let resetPasswordConfirm = $state('');
	let resetting = $state(false);
	let resetPasswordMismatch = $state(false);
	const resetForm = new FormDirtyState(() => ({
		password: resetPassword,
		passwordConfirm: resetPasswordConfirm
	}));

	function openResetDialog() {
		resetPasswordMismatch = false;
		resetPassword = '';
		resetPasswordConfirm = '';
		resetForm.markPristine();
		resetOpen = true;
	}

	async function handleResetSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (resetting) return;
		if (resetPassword !== resetPasswordConfirm) {
			resetPasswordMismatch = true;
			return;
		}
		resetPasswordMismatch = false;
		resetting = true;
		const { id, username: name } = target;
		const done = await errorDialog.attempt(() =>
			client.PUT('/api/v1/admin/users/{id}/password', {
				params: { path: { id } },
				body: { password: resetPassword }
			})
		);
		resetting = false;
		if (done === null) return;
		resetOpen = false;
		// 窓を閉じて終わり、画面には何も変わらないので知らせる (→ docs/ui.md「UI 全般」のトースト)。
		toast.success(m.admin_users_reset_password_done_toast({ username: name }));
	}

	// 消したユーザーの編集ページには戻れないので、一覧へ移る。消したものの変更は保存しようがないので、
	// 離れるときの確認は出さない。自分を消したときは、一覧の読み込みが 401 になってログインへ移る。
	async function handleDeleted() {
		await gotoWithoutLeaveConfirm(resolve('/admin/users'));
	}
</script>

<svelte:head><title>{pageTitle(target.username)}</title></svelte:head>

<div class="px-4 pt-4 md:px-6">
	<BackLink href={resolve('/admin/users')} label={m.admin_users_nav_title()} />
	<!-- パスワードの再設定は見出しの段のボタンに、削除は最後の「削除」の区画に置く (→ docs/ui.md「UI 全般」)。 -->
	<div class="mt-2 flex items-start gap-2">
		<h1 class={pageHeadingInRowClass}>{target.username}</h1>
		<Button class="shrink-0" variant="outline" size="sm" onclick={openResetDialog}>
			{m.admin_users_reset_password_button()}
		</Button>
	</div>
	<!-- 一覧で狭い幅のときに隠す作成日を、ここで確かめられるようにする (→ docs/ui.md「UI 全般」)。 -->
	<p class="text-sm text-muted-foreground">
		{m.admin_users_edit_created_at({ date: formatDate(target.createdAt) })}
	</p>
</div>

<div class={adminPageClass}>
	<!-- ユーザー名・権限を区画に分け、共有の編集ページと同じ石垣に並べる (→ docs/ui.md「編集ページの区画」)。 -->
	<div class={sectionGridClass} {@attach masonry}>
		<form
			onsubmit={handleUsernameSubmit}
			aria-labelledby="username-section-heading"
			class={sectionCardClass}
		>
			<!-- 区画の欄はユーザー名だけなので、見出しを欄のラベルにして同じ語を2度出さない。 -->
			<h2 id="username-section-heading">
				<Field.FieldLabel for="user-username" required class="text-base font-semibold"
					>{m.admin_users_dialog_username_label()}</Field.FieldLabel
				>
			</h2>
			<!-- ブラウザーのパスワードマネージャーが管理者自身の資格情報を入れないよう、自動入力を切る。 -->
			<Input id="user-username" autocomplete="off" bind:value={username} required />
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
			onsubmit={handleRoleSubmit}
			aria-labelledby="role-section-heading"
			class={sectionCardClass}
		>
			<h2 id="role-section-heading" class="text-base font-semibold">
				{m.admin_users_role_label()}
			</h2>
			<div class="flex flex-col gap-2">
				<RadioGroup.Root
					bind:value={() => role, (value) => (role = value as Role)}
					disabled={soleAdmin}
					aria-labelledby="role-section-heading"
				>
					{#each ROLES as option (option)}
						<Field.Field orientation="horizontal" data-disabled={soleAdmin} class="min-h-9">
							<RadioGroup.Item value={option} id="user-role-{option}" />
							<Field.FieldLabel for="user-role-{option}" class="font-normal"
								>{roleLabel(option)}</Field.FieldLabel
							>
						</Field.Field>
					{/each}
				</RadioGroup.Root>
				<!-- 押せない理由は、押せないときにだけ出す。 -->
				{#if soleAdmin}
					<p class="text-sm text-muted-foreground">{m.admin_users_last_admin_note()}</p>
				{/if}
			</div>
			<div>
				<LoadingButton
					type="submit"
					loading={sections.savingPart === 'role'}
					disabled={sections.saving || !roleForm.dirty}>{m.action_save()}</LoadingButton
				>
			</div>
		</form>
	</div>
	<!-- 最後の管理者は消せない。理由は区画の中にも書く。 -->
	<DeleteSection
		notes={[m.common_delete_section_irreversible(), m.admin_users_delete_section_note()]}
		disabledReason={soleAdmin ? m.admin_users_last_admin_note() : null}
		onclick={() => (deletingUser = target)}
	/>
</div>

<Dialog.Root bind:open={resetOpen}>
	<Dialog.Content
		interactOutsideBehavior={resetForm.dirty ? 'ignore' : 'close'}
		escapeKeydownBehavior={resetForm.dirty ? 'ignore' : 'close'}
	>
		<Dialog.Header>
			<Dialog.Title>{m.admin_users_reset_password_dialog_title()}</Dialog.Title>
			<Dialog.Description>
				{m.admin_users_reset_password_dialog_description({ username: target.username })}
			</Dialog.Description>
		</Dialog.Header>
		<form onsubmit={handleResetSubmit} class={dialogFormClass}>
			<Dialog.Body>
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
					<Field.Field data-invalid={resetPasswordMismatch}>
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
			</Dialog.Body>
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (resetOpen = false)}
					>{m.action_cancel()}</Button
				>
				<!-- ほかの入力のダイアログと同じく、送れる形になるまで押せなくする。 -->
				<LoadingButton
					type="submit"
					loading={resetting}
					disabled={resetPassword === '' || resetPasswordConfirm === ''}
					>{m.action_save()}</LoadingButton
				>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<UserDeleteDialog bind:user={deletingUser} ondeleted={handleDeleted} />

<LeaveConfirmDialog guard={sections.leaveGuard} ondiscard={() => sections.discard()} />

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
