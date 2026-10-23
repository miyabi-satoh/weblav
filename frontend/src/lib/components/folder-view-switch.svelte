<script lang="ts">
	import { resolve } from '$app/paths';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import ListFilterIcon from '@lucide/svelte/icons/list-filter';
	import * as m from '$lib/paraglide/messages.js';
	import { browseToggleItemClass } from '$lib/list-row';
	import { cn } from '$lib/utils';

	// 一覧の表示を使うフォルダーで、フォルダーの表示と一覧の表示を行き来する (→ docs/folders.md「2つの表示」)。
	// 押すと画面が移るので、ボタンでなくリンクの組にする。見た目はリストとタイルの切り替えに揃える。
	let { contentId, current }: { contentId: number; current: 'folder' | 'list' } = $props();

	let options = $derived([
		{
			value: 'folder' as const,
			label: m.folder_view_folder(),
			icon: FolderIcon,
			href: resolve('/folders/[id]', { id: String(contentId) })
		},
		{
			value: 'list' as const,
			label: m.folder_view_list(),
			icon: ListFilterIcon,
			href: resolve('/archives/[id]', { id: String(contentId) })
		}
	]);
</script>

<nav aria-label={m.folder_view_label()} class="inline-flex overflow-hidden rounded-md border">
	{#each options as option (option.value)}
		<a
			href={option.href}
			aria-current={option.value === current ? 'page' : undefined}
			data-state={option.value === current ? 'on' : 'off'}
			class={cn(
				'inline-flex items-center gap-2 px-4 text-sm not-first:border-l',
				browseToggleItemClass
			)}
		>
			<option.icon class="size-5" strokeWidth={1.8} />
			{option.label}
		</a>
	{/each}
</nav>
