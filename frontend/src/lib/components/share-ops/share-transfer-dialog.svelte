<script lang="ts" module>
	import type { components } from '$lib/api/schema';
	import type { PickedDir } from './share-dir-picker.svelte';
	import { accessAtLeast, type ShareTarget } from '$lib/share-ops';

	type TransferOp = components['schemas']['TransferOp'];

	/**
	 * 選んでいる場所に置けるか。足せない場所・それ自身の中・移動で元と同じフォルダー (何も変わらない) には置かない。
	 * 同じフォルダーへのコピーは、サーバーが番号を付けて写すので置ける。
	 */
	export function canPlaceAt(
		op: TransferOp,
		source: { shareId: number; dirPath: string; target: Pick<ShareTarget, 'path' | 'isDir'> },
		picked: PickedDir | null
	): boolean {
		if (picked === null || !accessAtLeast(picked.access, 'add')) return false;
		if (picked.shareId !== source.shareId) return true;
		const path = source.target.path;
		if (source.target.isDir && (picked.path === path || picked.path.startsWith(`${path}/`))) {
			return false;
		}
		return !(op === 'move' && picked.path === source.dirPath);
	}
</script>

<script lang="ts">
	/**
	 * 移動・コピー (→ docs/ui.md「共有の中の画面」)。置き先を選び、同じ名前があれば「上書きする」「番号を付ける」「飛ばす」を選ばせる。
	 * 始めたら裏で続く処理になるので、進み具合はルートのレイアウトの欄 (`share-jobs-panel.svelte`) に任せて閉じる。
	 */
	import { client } from '$lib/api/client';
	import { MESSAGES } from '$lib/api/errors';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import { jobLabel, shareJobs } from '$lib/share-jobs.svelte';
	import { lastSegment } from '$lib/share-ops';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import ShareDirPicker from './share-dir-picker.svelte';

	type ShareResponse = components['schemas']['ShareResponse'];
	type OnConflict = components['schemas']['OnConflict'];
	type TransferConflict = components['schemas']['TransferConflict'];
	type ListedAs = components['schemas']['ListedAs'];

	let {
		open = $bindable(),
		op,
		shareId,
		dirPath,
		target,
		shares,
		ondone
	}: {
		open: boolean;
		op: TransferOp;
		/** 動かす・写すものの共有と、それの入っているフォルダー。 */
		shareId: number;
		dirPath: string;
		target: ShareTarget;
		shares: ShareResponse[];
		/** 処理が終わった後の画面の更新。 */
		ondone: () => void | Promise<void>;
	} = $props();

	let picked = $state<PickedDir | null>(null);
	let conflict = $state<{
		entry: TransferConflict;
		trash: boolean;
		listed: ListedAs[];
	} | null>(null);
	/** 動かすと開けなくなるショートカット (→ docs/shares.md「場所の付け替え」)。 */
	let unlinked = $state<ListedAs[]>([]);
	/** 同じ名前が無くても、開けなくなるものがあれば確かめてから動かす。 */
	let confirmingMove = $state(false);
	let busy = $state<OnConflict | 'check' | null>(null);
	const errorDialog = new ErrorDialogState();

	// 足せる場所なら今いるフォルダーから、足せなければ共有の一覧から始める。
	let start = $derived(
		shares.some((share) => share.id === shareId && accessAtLeast(share.access, 'add'))
			? { shareId, path: dirPath }
			: null
	);

	let placeable = $derived(canPlaceAt(op, { shareId, dirPath, target }, picked));
	let destName = $derived(picked === null ? '' : lastSegment(picked.path, picked.shareName));

	$effect.pre(() => {
		if (open) {
			conflict = null;
			confirmingMove = false;
		}
	});

	function body(onConflict?: OnConflict) {
		const to = { shareId: picked!.shareId, path: picked!.path };
		const from = { shareId, paths: [target.path] };
		return onConflict === undefined ? { op, from, to } : { op, from, to, onConflict };
	}

	async function check() {
		if (!placeable || busy !== null || picked === null) return;
		busy = 'check';
		try {
			const result = await errorDialog.attempt(() =>
				client.POST('/api/v1/shares/transfer/check', { body: body() })
			);
			if (result?.data == null) return;
			const checked = result.data;
			// 移動は同じディスクの中なら場所を変えるだけなので、空きはコピーでだけ見る (サーバーと同じ)。
			if (op === 'copy' && checked.totalBytes > checked.availableBytes) {
				errorDialog.show(MESSAGES.insufficient_storage());
				return;
			}
			unlinked = checked.unlinked;
			const first = checked.conflicts[0];
			if (first !== undefined) {
				conflict = { entry: first, trash: checked.trash, listed: checked.listed };
				return;
			}
			if (unlinked.length > 0) {
				confirmingMove = true;
				return;
			}
		} finally {
			busy = null;
		}
		await begin('rename');
	}

	async function begin(onConflict: OnConflict) {
		if (busy !== null && busy !== 'check') return;
		busy = onConflict;
		try {
			const result = await errorDialog.attempt(() =>
				client.POST('/api/v1/shares/transfer', {
					body: body(onConflict) as components['schemas']['TransferRequest']
				})
			);
			if (result?.data == null) return;
			shareJobs.track(result.data.jobId, op, jobLabel(op, target.name), ondone);
			open = false;
		} finally {
			busy = null;
		}
	}
</script>

{#snippet unlinkedList()}
	<div class="flex flex-col gap-2 text-sm">
		<p>{m.shares_move_unlinked_note()}</p>
		<ul class="list-disc pl-5">
			{#each unlinked as listed (listed.contentId)}
				<li class="wrap-anywhere">{listed.title}</li>
			{/each}
		</ul>
	</div>
{/snippet}

<Dialog.Root bind:open>
	<Dialog.Content
		class={conflict === null && !confirmingMove
			? 'flex max-h-4/5 flex-col gap-0 p-0 sm:max-w-lg'
			: 'sm:max-w-lg'}
		escapeKeydownBehavior={busy !== null ? 'ignore' : 'close'}
		interactOutsideBehavior={busy !== null ? 'ignore' : 'close'}
	>
		{#if confirmingMove}
			<Dialog.Header>
				<Dialog.Title>{m.shares_move_unlinked_title({ name: target.name })}</Dialog.Title>
			</Dialog.Header>
			<Dialog.Body class="flex flex-col gap-2">
				{@render unlinkedList()}
			</Dialog.Body>
			<Dialog.Footer>
				<Button
					type="button"
					variant="outline"
					disabled={busy !== null}
					onclick={() => (open = false)}
				>
					{m.action_cancel()}
				</Button>
				<LoadingButton
					type="button"
					loading={busy === 'rename'}
					disabled={busy !== null}
					onclick={() => begin('rename')}>{m.shares_move_submit()}</LoadingButton
				>
			</Dialog.Footer>
		{:else if conflict === null}
			<Dialog.Header class="border-b p-4 text-left">
				<Dialog.Title>
					{op === 'move'
						? m.shares_move_title({ name: target.name })
						: m.shares_copy_title({ name: target.name })}
				</Dialog.Title>
			</Dialog.Header>
			<ShareDirPicker
				{shares}
				{start}
				disabledPath={target.isDir ? { shareId, path: target.path } : null}
				onpick={(dir) => (picked = dir)}
			/>
			{#if picked !== null && !accessAtLeast(picked.access, 'add')}
				<p class="border-t bg-muted/40 px-4 py-3 text-xs text-muted-foreground">
					{m.shares_picker_cannot_add()}
				</p>
			{/if}
			<!-- 中身に余白 (p-0) を持たせていないので、Dialog.Footer の負の余白を打ち消す (dir-picker.svelte と同じ)。 -->
			<Dialog.Footer class="mx-0 mb-0 bg-muted/40">
				<Button type="button" variant="outline" onclick={() => (open = false)}>
					{m.action_cancel()}
				</Button>
				<LoadingButton type="button" loading={busy !== null} disabled={!placeable} onclick={check}>
					{op === 'move' ? m.shares_move_submit() : m.shares_copy_submit()}
				</LoadingButton>
			</Dialog.Footer>
		{:else}
			<Dialog.Header>
				<Dialog.Title>
					{m.shares_conflict_title({ folder: destName, name: conflict.entry.name })}
				</Dialog.Title>
			</Dialog.Header>
			{#if unlinked.length > 0 && !(picked !== null && accessAtLeast(picked.access, 'all'))}
				<Dialog.Body class="flex flex-col gap-2">
					{@render unlinkedList()}
				</Dialog.Body>
			{/if}
			{#if picked !== null && accessAtLeast(picked.access, 'all')}
				<Dialog.Body class="flex flex-col gap-2">
					{#if unlinked.length > 0}
						{@render unlinkedList()}
					{/if}
					<p class="text-muted-foreground">
						{conflict.trash
							? m.shares_conflict_overwrite_trash({ name: conflict.entry.name })
							: m.shares_conflict_overwrite_permanent()}
					</p>
					{#if conflict.listed.length > 0}
						<div class="flex flex-col gap-2 text-sm">
							<p>{m.shares_conflict_listed_note()}</p>
							<ul class="list-disc pl-5">
								{#each conflict.listed as listed (listed.contentId)}
									<li class="wrap-anywhere">{listed.title}</li>
								{/each}
							</ul>
						</div>
					{/if}
				</Dialog.Body>
			{/if}
			<Dialog.Footer>
				<Button
					type="button"
					variant="outline"
					disabled={busy !== null}
					onclick={() => (open = false)}
				>
					{m.action_cancel()}
				</Button>
				<LoadingButton
					type="button"
					variant="outline"
					loading={busy === 'skip'}
					disabled={busy !== null}
					onclick={() => begin('skip')}>{m.shares_conflict_skip()}</LoadingButton
				>
				<LoadingButton
					type="button"
					variant="outline"
					loading={busy === 'rename'}
					disabled={busy !== null}
					onclick={() => begin('rename')}>{m.shares_conflict_rename()}</LoadingButton
				>
				<!-- 上書きは置き先で「フルアクセス」の人にだけ出す (サーバーも同じ判定で断る)。 -->
				{#if picked !== null && accessAtLeast(picked.access, 'all')}
					<LoadingButton
						type="button"
						variant="destructive"
						loading={busy === 'overwrite'}
						disabled={busy !== null}
						onclick={() => begin('overwrite')}>{m.shares_conflict_overwrite()}</LoadingButton
					>
				{/if}
			</Dialog.Footer>
		{/if}
	</Dialog.Content>
</Dialog.Root>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
