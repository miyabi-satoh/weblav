<script lang="ts">
	import type { ClassValue } from 'svelte/elements';
	import * as m from '$lib/paraglide/messages.js';
	import { listFilter } from '$lib/list-filter.svelte';
	import LabeledInputBox from '$lib/components/labeled-input-box.svelte';

	// 一覧のページ内の絞り込みの入力欄 (→ docs/ui.md「一覧の絞り込み」)。絞り込み中は差し色で強調する。
	let { class: className = 'w-40' }: { class?: ClassValue } = $props();

	const filter = listFilter();
</script>

{#if filter}
	<LabeledInputBox
		label={m.browse_filter_label()}
		clearLabel={m.browse_filter_clear_button()}
		active={filter.active}
		class={className}
		bind:value={() => filter.value, (next) => filter.set(next)}
	/>
{/if}
