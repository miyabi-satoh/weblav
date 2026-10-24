<script lang="ts">
	import { refreshAll } from '$app/navigation';
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

	type FolderMark = components['schemas']['FolderMark'];

	// フォルダーの編集ページの「公開・非公開を変えたもの」の区画 (→ docs/folders.md「公開と非公開」)。
	// 印を付けるのは閲覧の画面の行のメニューとアイテムのタブで、ここは取り消すための一覧。
	// 取り消すとその場で効くので、保存ボタンを置かない。
	let { contentId, marks }: { contentId: number; marks: FolderMark[] } = $props();

	let clearing = $state<number | null>(null);
	const errorDialog = new ErrorDialogState();

	async function clear(mark: FolderMark) {
		if (clearing !== null) return;
		clearing = mark.id;
		try {
			await errorDialog.attempt(
				() =>
					client.DELETE('/api/v1/contents/{id}/marks/{mark_id}', {
						params: { path: { id: contentId, mark_id: mark.id } }
					}),
				// 押した行が消えるので、トーストは出さない (→ docs/ui.md「押した所で結果が見えない操作は、トーストで知らせる」)。
				() => refreshAll()
			);
		} finally {
			clearing = null;
		}
	}
</script>

<section aria-labelledby="folder-marks-heading" class={sectionCardClass}>
	<h2 id="folder-marks-heading" class="text-base font-semibold">
		{m.contents_marks_title()}
	</h2>
	<ul class="flex flex-col">
		{#each marks as mark (mark.id)}
			{@const Icon = mark.isDir ? FolderIcon : FileIcon}
			<li class={['flex min-h-11 items-center justify-between gap-3', listItemClass]}>
				<span class="flex min-w-0 items-center gap-2 text-sm">
					<Icon class="size-4 shrink-0 text-muted-foreground" />
					<span class="wrap-anywhere">{mark.path}</span>
					<span class="shrink-0 text-muted-foreground"
						>{mark.public ? m.marks_public_label() : m.marks_private_label()}</span
					>
				</span>
				<Button
					variant="ghost"
					size="sm"
					disabled={clearing !== null}
					aria-label={m.contents_marks_clear_label({ name: mark.path })}
					onclick={() => void clear(mark)}
				>
					{m.contents_marks_clear_button()}
				</Button>
			</li>
		{/each}
	</ul>
</section>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
