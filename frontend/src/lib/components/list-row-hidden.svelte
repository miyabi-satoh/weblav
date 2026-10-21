<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { browseGutterClass } from '$lib/list-row';
	import * as m from '$lib/paraglide/messages.js';

	// 閲覧の一覧で、表示しないにした行の代わりに出す行。押した人がその場で取り消せるようにする
	// (AI が集めたリンク・フォルダー・アーカイブで共有する)。
	let {
		message,
		busy = false,
		onundo
	}: {
		/** 何をしたか (「〜を表示しないようにしました。」)。 */
		message: string;
		busy?: boolean;
		onundo: () => void;
	} = $props();
</script>

<div
	class={[
		'flex min-h-14 items-center justify-between gap-3 py-2 text-sm text-muted-foreground',
		browseGutterClass
	]}
	role="status"
>
	<span class="min-w-0">{message}</span>
	<Button variant="ghost" size="sm" disabled={busy} onclick={onundo}>
		{m.action_undo()}
	</Button>
</div>
