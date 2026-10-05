<script lang="ts">
	import type { Snippet } from 'svelte';
	import EllipsisVerticalIcon from '@lucide/svelte/icons/ellipsis-vertical';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as m from '$lib/paraglide/messages.js';
	import { actionMenuTriggerClass } from '$lib/table-columns';

	/**
	 * 管理画面の行末の ⋮ メニュー。項目は `DropdownMenu.Item` を children で渡す。
	 * `name` は行の名前。どの行のメニューかが読み上げで分かるよう、ボタンの名前に入れる (→ docs/ui.md「UI 全般」)。
	 */
	let { name, children }: { name: string; children: Snippet } = $props();
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger aria-label={m.common_row_actions({ name })} class={actionMenuTriggerClass}>
		<EllipsisVerticalIcon class="size-4" />
	</DropdownMenu.Trigger>
	<DropdownMenu.Content align="end">
		{@render children()}
	</DropdownMenu.Content>
</DropdownMenu.Root>
