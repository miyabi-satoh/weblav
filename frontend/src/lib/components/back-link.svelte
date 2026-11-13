<script lang="ts">
	// 編集ページの見出しの上の、一覧へ戻るリンク。`href` は呼び出し側が resolve() したもの。
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import { backLinkClass } from '#lib/back-link.ts';

	let { href, label }: { href: string; label: string } = $props();
</script>

<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- 呼び出し側が resolve() した値を受けるので静的に追えない (→ AGENTS.md「コードの規約」) -->
<a {href} class={backLinkClass}>
	<ChevronLeftIcon class="size-4" />
	<span class="underline underline-offset-4">{label}</span>
</a>
