<script lang="ts">
	/**
	 * 共有の中の操作のダイアログをまとめて置く。画面は `ShareOpsState` を作ってここに渡し、
	 * ⋮ (`share-row-menu-items.svelte`) と「追加」(`share-add-menu.svelte`) から開く。
	 */
	import { untrack } from 'svelte';
	import { client } from '$lib/api/client';
	import type { components } from '$lib/api/schema';
	import * as m from '$lib/paraglide/messages.js';
	import type { ShareOpsState } from '$lib/share-ops-state.svelte';
	import { lastSegment, type ShareTarget } from '$lib/share-ops';
	import ShareDeleteDialog from './share-delete-dialog.svelte';
	import ShareListOnHomeDialog from './share-list-on-home-dialog.svelte';
	import ShareNameDialog from './share-name-dialog.svelte';
	import ShareTransferDialog from './share-transfer-dialog.svelte';
	import ShareUploadDialog from './share-upload-dialog.svelte';

	type ListedAs = components['schemas']['ListedAs'];

	let { ops }: { ops: ShareOpsState } = $props();

	let dir = $derived(ops.dir());
	let dialog = $derived(ops.dialog);

	// 閉じるアニメーションの間も中身を出し続けるよう、対象は閉じても残す。
	let renameTarget = $state<ShareTarget | null>(null);
	/** 名前を変えると開けなくなるショートカット。確かめられなくても名前は変えられるので、そのときは空のまま。 */
	let renameUnlinked = $state<ListedAs[]>([]);
	// 移動・コピーは開くたびに作り直し、置き先の一覧を今いる場所から始める。
	let transfer = $state<{ op: 'move' | 'copy'; target: ShareTarget; key: number } | null>(null);
	$effect.pre(() => {
		if (dialog?.kind === 'rename') {
			renameTarget = dialog.target;
			renameUnlinked = [];
			void checkRename(dir.shareId, dialog.target.path);
		}
		if (dialog?.kind === 'transfer') {
			const key = untrack(() => (transfer?.key ?? 0) + 1);
			transfer = { op: dialog.op, target: dialog.target, key };
		}
	});

	async function checkRename(shareId: number, path: string) {
		const { data } = await client.POST('/api/v1/shares/{id}/rename/check', {
			params: { path: { id: shareId } },
			body: { path }
		});
		if (data && renameTarget?.path === path) renameUnlinked = data.unlinked;
	}

	type Kind = NonNullable<typeof dialog>['kind'];

	function isOpen(kind: Kind): boolean {
		return ops.dialog?.kind === kind;
	}

	/** そのダイアログが自分で閉じたときだけ閉じる (ほかのダイアログに替わった後で閉じない)。 */
	function closeIf(kind: Kind, open: boolean) {
		if (!open && ops.dialog?.kind === kind) ops.close();
	}

	function targetOf(kind: 'delete' | 'listOnHome'): ShareTarget | null {
		const current = ops.dialog;
		return current?.kind === kind ? current.target : null;
	}
</script>

<ShareUploadDialog
	bind:open={() => isOpen('upload'), (open) => closeIf('upload', open)}
	shareId={dir.shareId}
	path={dir.path}
	folderName={lastSegment(dir.path, dir.shareName)}
	maxBytes={dir.uploadMaxBytes}
	ondone={ops.onchanged}
/>

<ShareNameDialog
	bind:open={() => isOpen('newFolder'), (open) => closeIf('newFolder', open)}
	title={m.shares_new_folder_title()}
	submitLabel={m.shares_new_folder_submit()}
	request={(name) =>
		client.POST('/api/v1/shares/{id}/folders', {
			params: { path: { id: dir.shareId } },
			body: { path: dir.path, name }
		})}
	ondone={ops.onchanged}
/>

<ShareNameDialog
	bind:open={() => isOpen('rename'), (open) => closeIf('rename', open)}
	title={m.shares_rename_title({ name: renameTarget?.name ?? '' })}
	submitLabel={m.shares_rename_submit()}
	initialName={renameTarget?.name ?? ''}
	selectStem={renameTarget !== null && !renameTarget.isDir}
	request={(name) =>
		client.POST('/api/v1/shares/{id}/rename', {
			params: { path: { id: dir.shareId } },
			body: { path: renameTarget?.path ?? '', name }
		})}
	ondone={ops.onchanged}
>
	{#snippet notice()}
		{#if renameUnlinked.length > 0}
			<div class="flex flex-col gap-2 text-sm">
				<p>{m.shares_rename_unlinked_note()}</p>
				<ul class="list-disc pl-5">
					{#each renameUnlinked as listed (listed.contentId)}
						<li class="wrap-anywhere">{listed.title}</li>
					{/each}
				</ul>
			</div>
		{/if}
	{/snippet}
</ShareNameDialog>

{#if transfer !== null}
	{#key transfer.key}
		<ShareTransferDialog
			bind:open={() => isOpen('transfer'), (open) => closeIf('transfer', open)}
			op={transfer.op}
			shareId={dir.shareId}
			dirPath={dir.path}
			target={transfer.target}
			shares={ops.shares()}
			ondone={ops.onchanged}
		/>
	{/key}
{/if}

<ShareDeleteDialog
	bind:target={() => targetOf('delete'), (target) => closeIf('delete', target !== null)}
	shareId={dir.shareId}
	ondone={ops.onchanged}
/>

<ShareListOnHomeDialog
	bind:target={() => targetOf('listOnHome'), (target) => closeIf('listOnHome', target !== null)}
	shareId={dir.shareId}
	ondone={ops.onchanged}
/>
