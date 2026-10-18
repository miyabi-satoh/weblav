<script lang="ts">
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";

	// shadcn からの改変: FieldLabel と同じく、必須項目を示す赤いアスタリスクを表示する。
	// ラジオや複数の欄をまとめた項目は FieldLabel でなく FieldLegend で名前を付けるので、
	// 必須の印をこちらにも持たせ、呼び出し側で手書きしないようにする。`required` の意味は FieldLabel と同じ。
	let {
		ref = $bindable(null),
		class: className,
		variant = "legend",
		children,
		required = false,
		...restProps
	}: WithElementRef<HTMLAttributes<HTMLLegendElement>> & {
		variant?: "legend" | "label";
		required?: boolean;
	} = $props();
</script>

<legend
	bind:this={ref}
	data-slot="field-legend"
	data-variant={variant}
	class={cn("mb-1.5 font-medium data-[variant=label]:text-sm data-[variant=legend]:text-base", className)}
	{...restProps}
>
	{@render children?.()}
	{#if required}
		<span class="ms-2 text-destructive" aria-hidden="true">*</span>
	{/if}
</legend>
