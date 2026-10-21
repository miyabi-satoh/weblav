<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { invalidateAll } from '$app/navigation';
	import FileIcon from '@lucide/svelte/icons/file';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import { client } from '$lib/api/client';
	import type { components } from '$lib/api/schema';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { listItemClass } from '$lib/list-row';
	import { sectionCardClass } from '$lib/page-layout';
	import * as m from '$lib/paraglide/messages.js';
	import { Button } from '$lib/components/ui/button';
	import ErrorDialog from '$lib/components/error-dialog.svelte';

	type HiddenEntry = components['schemas']['FolderHiddenEntry'];

	// フォルダーの編集ページの「表示しないもの」の区画 (→ docs/folders.md「表示しないもの」)。
	// 表示しないにするのは閲覧の画面の行のメニューで、ここは取り消すための一覧。
	// 「表示に戻す」はその場で効くので、保存ボタンを置かない。
	let { contentId, entries }: { contentId: number; entries: HiddenEntry[] } = $props();

	let restoring = $state<number | null>(null);
	const errorDialog = new ErrorDialogState();

	async function restore(entry: HiddenEntry) {
		if (restoring !== null) return;
		restoring = entry.id;
		try {
			await errorDialog.attempt(
				() =>
					client.DELETE('/api/v1/contents/{id}/hidden-entries/{entry_id}', {
						params: { path: { id: contentId, entry_id: entry.id } }
					}),
				async () => {
					toast.success(m.contents_hidden_restored_toast());
					await invalidateAll();
				}
			);
		} finally {
			restoring = null;
		}
	}
</script>

<section aria-labelledby="folder-hidden-heading" class={sectionCardClass}>
	<h2 id="folder-hidden-heading" class="text-base font-semibold">
		{m.contents_hidden_title()}
	</h2>
	<ul class="flex flex-col">
		{#each entries as entry (entry.id)}
			{@const Icon = entry.isDir ? FolderIcon : FileIcon}
			<li class={['flex min-h-11 items-center justify-between gap-3', listItemClass]}>
				<span class="flex min-w-0 items-center gap-2 text-sm">
					<Icon class="size-4 shrink-0 text-muted-foreground" />
					<span class="wrap-anywhere">{entry.path}</span>
				</span>
				<Button
					variant="ghost"
					size="sm"
					disabled={restoring !== null}
					aria-label={m.contents_hidden_restore_label({ name: entry.path })}
					onclick={() => void restore(entry)}
				>
					{m.contents_hidden_restore_button()}
				</Button>
			</li>
		{/each}
	</ul>
</section>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
