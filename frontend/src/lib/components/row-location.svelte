<script lang="ts">
	import FolderIcon from '@lucide/svelte/icons/folder';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { browseLayout } from '$lib/browse-layout.svelte';
	import { browseRowAboveClass } from '$lib/list-row';
	import { rowLocationText, type RowLocation } from '$lib/row-location';
	import * as m from '$lib/paraglide/messages.js';

	// 検索の結果の行の2段目に出す、どこにあるか。押すと階層を全部並べ、選んだ段の画面を開く
	// (→ docs/search.md「画面」)。行のリンクの上に重ねて置く (→ $lib/list-row.ts)。
	let {
		location,
		clip = browseLayout.tile
	}: {
		location: RowLocation;
		/** 1行に収め、収まらなければ親より上を「…」で切る。省くと、タイルで切り、リストでは折り返す。 */
		clip?: boolean;
	} = $props();

	let text = $derived(rowLocationText(location));
</script>

<DropdownMenu.Root>
	<!-- 押せる範囲は、行のリンクと取り合わないよう、行の下の余白の側へ広げる (→ docs/ui.md「UI 全般」)。 -->
	<DropdownMenu.Trigger
		aria-label={m.row_location_button({ path: text.full })}
		class={[
			browseRowAboveClass,
			'inline-flex max-w-full min-w-0 gap-1 rounded-sm text-left align-top underline-offset-2 after:absolute after:-inset-x-2 after:-top-1 after:-bottom-2.5 hover:text-foreground hover:underline',
			clip ? 'items-center' : 'items-start'
		]}
	>
		<!-- 行の高さの中で文字の1行目に揃える。 -->
		<FolderIcon class={['size-3.5 shrink-0', !clip && 'mt-px sm:mt-0.75']} strokeWidth={1.8} />
		{#if clip}
			<span class="flex min-w-0">
				{#if text.front !== ''}
					<span class="min-w-0 truncate">{text.front}</span>
				{/if}
				<!-- 頭の空白 (` / 親`) が flex の項目の端で詰められないよう、空白をそのまま残す。 -->
				<span class="max-w-full shrink-0 overflow-hidden text-ellipsis whitespace-pre"
					>{text.tail}</span
				>
			</span>
		{:else}
			<span class="min-w-0 wrap-anywhere">{text.front}{text.tail}</span>
		{/if}
	</DropdownMenu.Trigger>
	<DropdownMenu.Content align="start" class="max-w-sm">
		<DropdownMenu.Label>{m.row_location_menu_label()}</DropdownMenu.Label>
		{#each location.levels as level, index (index)}
			{@const Icon = level.icon}
			<DropdownMenu.Item>
				{#snippet child({ props })}
					<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- 呼び出し側が組み立てた href で静的に追えない (→ AGENTS.md「コードの規約」) -->
					<a href={level.href} {...props}>
						<Icon class="text-muted-foreground" strokeWidth={1.8} />
						<span class="min-w-0 wrap-anywhere">{level.label}</span>
					</a>
				{/snippet}
			</DropdownMenu.Item>
		{/each}
	</DropdownMenu.Content>
</DropdownMenu.Root>
