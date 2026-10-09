<script lang="ts">
	import ListIcon from '@lucide/svelte/icons/list';
	import LayoutGridIcon from '@lucide/svelte/icons/layout-grid';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import { browseLayout, type BrowseLayout } from '$lib/browse-layout.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import { browseToggleItemClass } from '$lib/list-row';

	const options: { value: BrowseLayout; label: () => string; icon: typeof ListIcon }[] = [
		{ value: 'list', label: m.browse_layout_list, icon: ListIcon },
		{ value: 'tile', label: m.browse_layout_tile, icon: LayoutGridIcon }
	];
</script>

<!-- 閲覧側の一覧をリストとタイルで切り替える (→ docs/ui.md「UI 全般」)。高さは並び順のドロップダウンと揃える。 -->
<ToggleGroup.Root
	type="single"
	variant="outline"
	aria-label={m.browse_layout_label()}
	bind:value={
		() => browseLayout.value,
		// 選んでいる側をもう一度押すと選択が外れる (空の値が来る) が、どちらかは常に選んでおく。
		(next) => {
			if (next === 'list' || next === 'tile') browseLayout.value = next;
		}
	}
>
	{#each options as option (option.value)}
		<ToggleGroup.Item
			value={option.value}
			aria-label={option.label()}
			class={['w-12', browseToggleItemClass]}
		>
			<option.icon class="size-5" strokeWidth={1.8} />
		</ToggleGroup.Item>
	{/each}
</ToggleGroup.Root>
