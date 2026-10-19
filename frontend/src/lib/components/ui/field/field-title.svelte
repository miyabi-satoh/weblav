<script lang="ts">
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";

	// shadcn からの改変: field-label.svelte と同じく、必須項目を示す赤いアスタリスクを表示する。
	// 入力欄でない項目 (選んで決める欄など) にも必須の印が要るため。
	let {
		ref = $bindable(null),
		class: className,
		children,
		required = false,
		...restProps
	}: WithElementRef<HTMLAttributes<HTMLDivElement>> & { required?: boolean } = $props();
</script>

<div
	bind:this={ref}
	data-slot="field-label"
	class={cn("gap-2 text-sm leading-snug font-medium group-data-[disabled=true]/field:opacity-50 flex w-fit items-center leading-snug", className)}
	{...restProps}
>
	{@render children?.()}
	{#if required}
		<span class="text-destructive" aria-hidden="true">*</span>
	{/if}
</div>
