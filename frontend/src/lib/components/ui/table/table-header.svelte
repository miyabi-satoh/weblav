<script lang="ts">
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";

	let {
		ref = $bindable(null),
		class: className,
		children,
		...restProps
	}: WithElementRef<HTMLAttributes<HTMLTableSectionElement>> = $props();
</script>

<!-- shadcn からの改変: 見出しの下は面の境界なので、table-row.svelte の divider を border に戻す
     (→ docs/ui.md「UI 全般」)。 -->
<thead
	bind:this={ref}
	data-slot="table-header"
	class={cn("[&_tr]:border-b [&_tr]:border-border", className)}
	{...restProps}
>
	{@render children?.()}
</thead>
