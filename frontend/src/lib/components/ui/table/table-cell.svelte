<script lang="ts">
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLTdAttributes } from "svelte/elements";

	let {
		ref = $bindable(null),
		class: className,
		children,
		...restProps
	}: WithElementRef<HTMLTdAttributes> = $props();
</script>

<!-- shadcn からの改変: 管理画面のテーブル行の最小高を 44px にする (→ docs/ui.md「UI 全般」)。
     td の height は表のレイアウトでは最小高として働くので、中身が高い行はそのまま伸びる。 -->
<td bind:this={ref} data-slot="table-cell" class={cn("h-11 p-2 align-middle whitespace-nowrap [&:has([role=checkbox])]:pr-0", className)} {...restProps}>
	{@render children?.()}
</td>
