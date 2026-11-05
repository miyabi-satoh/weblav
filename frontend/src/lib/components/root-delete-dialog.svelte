<script lang="ts">
	/**
	 * 共有の削除の確認。共有の一覧の⋮と、共有の編集ページの⋮で同じものを出す。
	 */
	import { page } from '$app/state';
	import { client } from '$lib/api/client';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { formatNumber } from '$lib/format';
	import * as m from '$lib/paraglide/messages.js';
	import { canAddRoots } from '$lib/root-location';
	import ConfirmDialog from '$lib/components/confirm-dialog.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import type { components } from '$lib/api/schema';

	type Root = components['schemas']['RootResponse'];

	let {
		root = $bindable(),
		ondeleted
	}: {
		/** 消す共有。入れると開き、閉じると `null` に戻す。 */
		root: Root | null;
		ondeleted: () => void | Promise<void>;
	} = $props();

	// 追加し直せるのはサーバーの PC からだけなので、ほかの端末ではそう添える。
	let canAdd = $derived(canAddRoots(page.url.hostname));
	let deleting = $state(false);
	const errorDialog = new ErrorDialogState();

	async function handleDelete() {
		if (root === null || deleting) return;
		deleting = true;
		const target = root;
		try {
			await errorDialog.attempt(
				() => client.DELETE('/api/v1/admin/roots/{id}', { params: { path: { id: target.id } } }),
				() => ondeleted()
			);
		} finally {
			root = null;
			deleting = false;
		}
	}
</script>

<ConfirmDialog
	bind:open={
		() => root !== null,
		(open) => {
			if (!open) root = null;
		}
	}
	title={m.admin_roots_delete_confirm_title()}
	busy={deleting}
	onconfirm={handleDelete}
>
	{m.admin_roots_delete_confirm_description({ name: root?.name ?? '' })}
	<!-- パスは長く、区切り以外に折り返せる場所が無いので、文から分けて小さく出し、どこででも折る
	     (`break-words` では最小幅が縮まず、狭い幅でダイアログを押し広げる)。 -->
	<span class="my-1 block text-xs wrap-anywhere">{root?.path ?? ''}</span>
	{#if root !== null && root.removedContentCount > 0}
		{m.admin_roots_delete_confirm_contents_note({
			count: formatNumber(root.removedContentCount)
		})}
	{/if}
	{#if !canAdd}
		{m.admin_roots_delete_confirm_local_note()}
	{/if}
</ConfirmDialog>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
