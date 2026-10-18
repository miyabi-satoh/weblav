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

<!-- shadcn からの追加: 見出しと足の間の本体。ダイアログが画面の高さに届いたら、ここだけをスクロールさせて
     見出しと確定のボタンを見せたままにする (→ docs/ui.md「UI 全般」)。
     左右の余白をダイアログの端まで広げ (-mx-4 px-4)、スクロールバーを端に寄せる。
     上下の 4px (-my-1 py-1) は、端の入力欄のフォーカスの輪がスクロールの縁で切れないため。 -->
<div
	bind:this={ref}
	data-slot="dialog-body"
	class={cn("-mx-4 -my-1 flex min-h-0 flex-col gap-6 overflow-y-auto px-4 py-1", className)}
	{...restProps}
>
	{@render children?.()}
</div>
