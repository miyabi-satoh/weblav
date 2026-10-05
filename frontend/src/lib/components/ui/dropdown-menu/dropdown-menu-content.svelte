<script lang="ts">
	import { DropdownMenu as DropdownMenuPrimitive } from "bits-ui";
	import { cn, type WithoutChildrenOrChild } from "$lib/utils.js";
	import DropdownMenuPortal from "./dropdown-menu-portal.svelte";
	import type { ComponentProps } from "svelte";

	let {
		ref = $bindable(null),
		sideOffset = 4,
		align = "start",
		portalProps,
		class: className,
		...restProps
	}: DropdownMenuPrimitive.ContentProps & {
		portalProps?: WithoutChildrenOrChild<ComponentProps<typeof DropdownMenuPortal>>;
	} = $props();
</script>

<!-- shadcn からの改変: 幅を開くボタンの幅 (anchor-width) ではなく中身に合わせる。行の ⋮ のような小さいボタンだと
     最小幅 (8rem) に収まり、「パスワード再設定...」などの項目が折り返すため。最小幅も含めて画面の端は越えない。 -->
<DropdownMenuPortal {...portalProps}>
	<DropdownMenuPrimitive.Content
		bind:ref
		data-slot="dropdown-menu-content"
		{sideOffset}
		{align}
		class={cn(
			"min-w-[min(8rem,var(--bits-dropdown-menu-content-available-width))] rounded-lg bg-popover p-1 text-popover-foreground shadow-md ring-1 ring-foreground/10 duration-100 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95 data-[side=inline-end]:slide-in-from-left-2 data-[side=inline-start]:slide-in-from-right-2 z-50 w-max max-w-(--bits-dropdown-menu-content-available-width) overflow-x-hidden overflow-y-auto outline-none data-closed:overflow-hidden",
			className
		)}
		{...restProps}
	/>
</DropdownMenuPortal>
