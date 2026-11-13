<script lang="ts">
	/**
	 * ユーザーの削除の確認。ユーザーの一覧の⋮と、ユーザーの編集ページの「削除」の区画で同じものを出す。
	 */
	import { client } from '#lib/api/client.ts';
	import { ErrorDialogState } from '#lib/error-dialog-state.svelte.ts';
	import { formatNumber } from '#lib/format.ts';
	import * as m from '#lib/paraglide/messages.js';
	import ConfirmDialog from '#lib/components/confirm-dialog.svelte';
	import ErrorDialog from '#lib/components/error-dialog.svelte';
	import type { components } from '#lib/api/schema.js';

	type UserListItem = components['schemas']['UserListItem'];

	let {
		user = $bindable(),
		ondeleted
	}: {
		/** 消すユーザー。入れると開き、閉じると `null` に戻す。 */
		user: UserListItem | null;
		ondeleted: (deleted: UserListItem) => void | Promise<void>;
	} = $props();

	let deleting = $state(false);
	const errorDialog = new ErrorDialogState();

	async function handleDelete() {
		if (user === null || deleting) return;
		deleting = true;
		const target = user;
		try {
			await errorDialog.attempt(
				() => client.DELETE('/api/v1/admin/users/{id}', { params: { path: { id: target.id } } }),
				() => ondeleted(target)
			);
		} finally {
			user = null;
			deleting = false;
		}
	}
</script>

<ConfirmDialog
	bind:open={
		() => user !== null,
		(open) => {
			if (!open) user = null;
		}
	}
	title={m.admin_users_delete_confirm_title()}
	busy={deleting}
	onconfirm={handleDelete}
>
	{m.admin_users_delete_confirm_description({ username: user?.username ?? '' })}
	{#if user !== null && user.privateContentCount > 0}
		<br />
		{m.admin_users_delete_confirm_private_note({
			count: formatNumber(user.privateContentCount)
		})}
	{/if}
</ConfirmDialog>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
