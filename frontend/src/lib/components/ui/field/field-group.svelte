<script lang="ts">
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";

	let {
		ref = $bindable(null),
		class: className,
		children,
		...restProps
	}: WithElementRef<HTMLAttributes<HTMLDivElement>> = $props();
</script>

<!-- 改変: 項目と項目の間を既定の gap-5 (20px) から gap-6 (24px) にする。余白の決めごとの「組と組」に揃えるため (→ docs/ui.md「UI 全般」)。 -->
<div
	bind:this={ref}
	data-slot="field-group"
	class={cn(
		"gap-6 data-[slot=checkbox-group]:gap-3 *:data-[slot=field-group]:gap-4 group/field-group @container/field-group flex w-full flex-col",
		className
	)}
	{...restProps}
>
	{@render children?.()}
</div>
