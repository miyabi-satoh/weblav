<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import * as m from '$lib/paraglide/messages.js';

	// 区切って取っている一覧が、取り始めた後に変わった (ほかの人が公開を切り替えた・再スキャンした) ことの知らせ
	// (→ docs/archive.md「一覧の API を区切って返す」)。勝手に取り直すと見ている行が動くので、押したときだけ取り直す。
	let { onreload }: { onreload: () => void } = $props();
</script>

<div
	role="status"
	class="mt-2 flex flex-wrap items-center justify-between gap-x-3 gap-y-1 rounded-md border bg-muted/50 px-3 py-2 text-sm"
>
	<span>{m.archive_view_stale()}</span>
	<Button type="button" variant="outline" size="sm" onclick={onreload}>
		{m.archive_view_reload_button()}
	</Button>
</div>
