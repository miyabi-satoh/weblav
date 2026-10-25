<script lang="ts">
	import type { ArchivePagedList } from '$lib/archive-paged-list.svelte';
	import { ARCHIVE_CHUNK_SIZE } from '$lib/archive-paged-list.svelte';
	import { browseRowClass, browseRowTextClass } from '$lib/list-row';
	import { Button } from '$lib/components/ui/button';
	import * as m from '$lib/paraglide/messages.js';

	// 区切って取るアーカイブの一覧の、まだ取っていない行と、消えた・非公開になった行
	// (→ docs/archive.md「画面が見える辺りを取りに行く」)。行と同じ高さで場所を取り、取り終えたら行に替わる。
	let {
		list,
		index,
		missing = false
	}: {
		list: ArchivePagedList<unknown>;
		index: number;
		/** 結果を作った後に消えた・非公開になった行。名前は出さない。 */
		missing?: boolean;
	} = $props();

	let chunk = $derived(Math.floor(index / ARCHIVE_CHUNK_SIZE));
	let failed = $derived(list.failed.has(chunk));

	// 描かれた (見えてきた) ときに、この行の区切りを取りに行く。
	$effect(() => {
		if (!missing) list.ensure(index);
	});
</script>

<div class={browseRowClass()} aria-busy={!missing && !failed}>
	<span class={browseRowTextClass()}>
		{#if missing}
			<span class="block text-sm text-muted-foreground">{m.archive_view_missing_row()}</span>
		{:else if failed}
			<span class="flex flex-wrap items-center gap-x-3 gap-y-1">
				<span class="text-sm text-muted-foreground">{m.archive_view_chunk_failed()}</span>
				<Button type="button" variant="outline" size="sm" onclick={() => list.retry(chunk)}>
					{m.action_retry()}
				</Button>
			</span>
		{:else}
			<!-- 行のタイトルと2段目の場所。読み上げには aria-busy で読み込み中と伝える。 -->
			<span class="block h-6 w-2/3 rounded bg-muted" aria-hidden="true"></span>
			<span class="mt-1 block h-4 w-1/3 rounded bg-muted" aria-hidden="true"></span>
		{/if}
	</span>
</div>
