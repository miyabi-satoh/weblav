<script lang="ts">
	/** `VirtualPages` で分けたページの、前後へ移るボタンと今の位置。 */
	import { Button } from '#lib/components/ui/button/index.ts';
	import { formatNumber } from '#lib/format.ts';
	import * as m from '#lib/paraglide/messages.js';
	import type { VirtualPages } from '#lib/virtual-pages.svelte.ts';

	let {
		pages,
		count,
		ongo
	}: {
		pages: VirtualPages;
		/** 1ページの件数。ボタンに「何件ずつ」と出す。 */
		count: number;
		ongo: (next: number) => void;
	} = $props();
</script>

<div class="flex flex-wrap items-center justify-center gap-3 py-4">
	<Button
		type="button"
		variant="outline"
		disabled={pages.page === 0}
		onclick={() => ongo(pages.page - 1)}
	>
		{m.archive_view_previous_page({ count: formatNumber(count) })}
	</Button>
	<span class="text-sm text-muted-foreground">
		{m.archive_view_page_position({ page: pages.page + 1, pages: pages.pageCount })}
	</span>
	<Button
		type="button"
		variant="outline"
		disabled={pages.page >= pages.pageCount - 1}
		onclick={() => ongo(pages.page + 1)}
	>
		{m.archive_view_next_page({ count: formatNumber(count) })}
	</Button>
</div>
