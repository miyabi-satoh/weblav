<script lang="ts">
	import Loader2Icon from '@lucide/svelte/icons/loader-2';
	import { cn } from "$lib/utils.js";
	import type { SVGAttributes } from "svelte/elements";

	let {
		class: className,
		role = "status",
		// we add name, color, and stroke for compatibility with different icon libraries props
		name,
		color,
		stroke,
		"aria-label": ariaLabel = "Loading",
		...restProps
	}: SVGAttributes<SVGSVGElement> = $props();

	// FIX: shadcn からの改変。指定されていない name / color / stroke は属性ごと渡さない。
	// @lucide/svelte は渡された属性を既定の属性 (stroke="currentColor") の上に重ねるので、
	// stroke={undefined} を渡すと線が消えてスピナーが見えなくなる。
	const iconProps = $derived({
		...(name == null ? {} : { name }),
		...(color == null ? {} : { color }),
		...(stroke == null ? {} : { stroke }),
	});
</script>

<Loader2Icon {role} {...iconProps} aria-label={ariaLabel} class={cn("size-4 animate-spin", className)} {...restProps} />
