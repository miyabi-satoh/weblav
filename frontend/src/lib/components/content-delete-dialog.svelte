<script lang="ts">
	/**
	 * コンテンツを消す前の確認と、消す要求。コンテンツ管理の一覧と編集ページの両方から使う (→ docs/ui.md「UI 全般」)。
	 * `content` に対象を入れると開き、閉じると `null` に戻す。
	 */
	import { client } from '$lib/api/client';
	import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
	import { visibilityLabel } from '$lib/content-labels';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { formatNumber } from '$lib/format';
	import { childrenLoosenedByDeletingGroup } from '$lib/visibility';
	import * as m from '$lib/paraglide/messages.js';
	import type { components } from '$lib/api/schema';
	import ConfirmDialog from '$lib/components/confirm-dialog.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';

	type Content = components['schemas']['AdminContentResponse'];

	type Props = {
		content: Content | null;
		/** 見える範囲が広がる子を探すのに使う、管理画面の一覧の全件。 */
		contents: readonly Content[];
		contentById: ReadonlyMap<number, Content>;
		/** 消し終えた後の画面の更新 (一覧を取り直す・一覧へ移る)。 */
		ondeleted: () => void | Promise<void>;
	};

	let { content = $bindable(), contents, contentById, ondeleted }: Props = $props();

	let deleting = $state(false);
	const errorDialog = new ErrorDialogState();

	// グループの削除でルート直下へ移り、見える範囲が広がる子。公開範囲は書き換えないので知らせる。
	let loosenedChildren = $derived(
		content?.type === 'group'
			? childrenLoosenedByDeletingGroup(content.id, contents, contentById)
			: []
	);
	// 子が多いと確定ボタンが画面の外へ押し出されるので、並べるのは先頭の数件にとどめる。
	const LOOSENED_CHILDREN_SHOWN = 5;

	async function handleDelete() {
		if (deleting || content === null) return;
		deleting = true;
		const target = content;

		try {
			const { error, response } = await client.DELETE('/api/v1/contents/{id}', {
				params: { path: { id: target.id } }
			});
			if (!response.ok) {
				errorDialog.show(errorMessage(error));
				return;
			}
			// 画面を更新し終えてから閉じる。先に閉じると、移り終えるまで消したものを操作できてしまうため。
			// 更新に失敗しても閉じる。消えたものをもう一度消させないため。
			try {
				await ondeleted();
			} finally {
				content = null;
			}
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			deleting = false;
		}
	}
</script>

<ConfirmDialog
	bind:open={
		() => content !== null,
		(open) => {
			if (!open) content = null;
		}
	}
	title={m.contents_delete_confirm_title()}
	busy={deleting}
	onconfirm={handleDelete}
>
	{m.contents_delete_confirm_description({ title: content?.title ?? '' })}
	{#if content?.type === 'group'}
		<br />
		{m.contents_delete_confirm_group_note()}
	{/if}
	{#if loosenedChildren.length > 0}
		<br />
		{m.contents_delete_confirm_loosened_note({ count: formatNumber(loosenedChildren.length) })}
		<ul class="mt-1 list-disc pl-5 text-left">
			{#each loosenedChildren.slice(0, LOOSENED_CHILDREN_SHOWN) as { content: child, before } (child.id)}
				<li>
					{m.contents_delete_confirm_loosened_item({
						title: child.title,
						before: visibilityLabel(before),
						after: visibilityLabel(child.visibility)
					})}
				</li>
			{/each}
			{#if loosenedChildren.length > LOOSENED_CHILDREN_SHOWN}
				<li>
					{m.contents_delete_confirm_loosened_more({
						count: formatNumber(loosenedChildren.length - LOOSENED_CHILDREN_SHOWN)
					})}
				</li>
			{/if}
		</ul>
	{/if}
</ConfirmDialog>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
