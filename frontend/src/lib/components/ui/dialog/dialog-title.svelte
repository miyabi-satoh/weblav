<script lang="ts">
	import { Dialog as DialogPrimitive } from "bits-ui";
	import { cn } from "$lib/utils.js";

	let {
		ref = $bindable(null),
		class: className,
		...restProps
	}: DialogPrimitive.TitleProps = $props();
</script>

<!-- shadcn からの改変: 右上の閉じるボタン (dialog-content.svelte) は浮いているので、長い見出し (英語の
     表示など) が折り返さずにその下へ潜り込む。右に閉じるボタンの分を空け、2行になっても行が詰まらない行送りにする。 -->
<DialogPrimitive.Title
	bind:ref
	data-slot="dialog-title"
	class={cn("pr-6 text-base leading-snug font-medium", className)}
	{...restProps}
/>
