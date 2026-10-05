<script lang="ts">
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";

	let {
		ref = $bindable(null),
		class: className,
		children,
		...restProps
	}: WithElementRef<HTMLAttributes<HTMLTableRowElement>> = $props();
</script>

<!-- shadcn からの改変: 行の区切りを border ではなく divider の色にする (→ docs/ui.md「UI 全般」)。
     見出し行は table-header.svelte 側で border に戻している。 -->
<tr bind:this={ref} data-slot="table-row" class={cn("border-b border-divider transition-colors hover:bg-muted/50 data-[state=selected]:bg-muted", className)} {...restProps}>
	{@render children?.()}
</tr>
