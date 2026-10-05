<script lang="ts">
	import { Label } from "$lib/components/ui/label/index.js";
	import { cn } from "$lib/utils.js";
	import type { ComponentProps } from "svelte";

	// shadcn からの改変: 必須項目を示す赤いアスタリスクを表示する。
	// `required`はHTMLのrequired属性とは独立していて、呼び出し側が「データとして
	// ドメイン上必須かどうか」で判断して渡す(単なる入力必須かどうかではない)。
	let {
		ref = $bindable(null),
		class: className,
		children,
		required = false,
		...restProps
	}: ComponentProps<typeof Label> & { required?: boolean } = $props();
</script>

<Label
	bind:ref
	data-slot="field-label"
	class={cn(
		"gap-2 leading-snug group-data-[disabled=true]/field:opacity-50 has-data-checked:border-primary/30 has-data-checked:bg-primary/5 has-[>[data-slot=field]]:rounded-lg has-[>[data-slot=field]]:border has-[>[data-slot=field]]:not-has-[:disabled,[data-disabled]]:hover:bg-muted/50 has-[>[data-slot=field]]:has-[:focus-visible]:border-ring has-[>[data-slot=field]]:has-[:focus-visible]:ring-3 has-[>[data-slot=field]]:has-[:focus-visible]:ring-ring/50 *:data-[slot=field]:p-2.5 dark:has-data-checked:border-primary/20 dark:has-data-checked:bg-primary/10 group/field-label peer/field-label flex w-fit leading-snug",
		"has-[>[data-slot=field]]:w-full has-[>[data-slot=field]]:flex-col",
		className
	)}
	{...restProps}
>
	{@render children?.()}
	{#if required}
		<span class="text-destructive" aria-hidden="true">*</span>
	{/if}
</Label>
