<script lang="ts">
	import type { Snippet } from 'svelte';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';

	// 新規タブで開く行のタイトルの、文字のすぐ後ろに置く印 (→ docs/ui.md「UI 全般」)。
	// 行の右端は ⋮ のメニューが使うので、印はタイトルの側に寄せる。リストとタイルで同じ位置にする。
	// 印は装飾扱い (aria-hidden) で、新規タブで開くことは、リンクの中の読み上げ用の文字で伝える。
	let { children }: { children: Snippet } = $props();
</script>

<span class="flex min-w-0 items-start gap-1.5">
	<span class="min-w-0">{@render children()}</span>
	<!-- タイトルの1行目 (leading-6) の中ほどに揃える。 -->
	<ExternalLinkIcon
		class="mt-1 size-4 shrink-0 text-muted-foreground"
		strokeWidth={1.8}
		aria-hidden="true"
	/>
</span>
