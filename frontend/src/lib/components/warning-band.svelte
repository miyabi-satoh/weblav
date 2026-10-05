<script lang="ts">
	import type { Snippet } from 'svelte';
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
	import { cn } from '$lib/utils';

	let { class: className, children }: { class?: string; children: Snippet } = $props();
</script>

<!-- 注意を促す帯は差し色ではなく警告色で塗る (→ docs/ui.md「UI 全般」)。 -->
<div
	class={cn(
		'flex items-start gap-3 border-y border-warning-border bg-warning px-4 py-4 text-warning-foreground',
		className
	)}
>
	<TriangleAlertIcon class="mt-0.5 size-5 shrink-0 text-warning-icon" />
	<div class="flex-1">
		{@render children()}
	</div>
</div>
