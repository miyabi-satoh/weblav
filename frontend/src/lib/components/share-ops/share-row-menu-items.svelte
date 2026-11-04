<script lang="ts">
	/**
	 * 共有の中の行の ⋮ に並べる操作 (→ docs/ui.md「共有の中の画面」)。押してから断られるものは出さない (`shareRowActions`)。
	 * `RowActionsMenu` の中に置く。フォルダーのコンテンツの画面でも同じものを出す。
	 */
	import { shareDownloadHref } from '$lib/api/urls';
	import * as m from '$lib/paraglide/messages.js';
	import type { ShareOpsState } from '$lib/share-ops-state.svelte';
	import { shareRowActions, type ShareAction, type ShareTarget } from '$lib/share-ops';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';

	let {
		ops,
		target,
		omit = []
	}: {
		ops: ShareOpsState;
		target: ShareTarget;
		/** 出さない操作 (その画面で別の口があるもの)。 */
		omit?: ShareAction[];
	} = $props();

	let dir = $derived(ops.dir());
	let actions = $derived(
		shareRowActions(dir.access, target, ops.copyDestinationExists).filter(
			(action) => !omit.includes(action)
		)
	);
</script>

{#each actions as action (action)}
	{#if action === 'rename'}
		<DropdownMenu.Item onSelect={() => ops.open({ kind: 'rename', target })}>
			{m.shares_rename_menu()}
		</DropdownMenu.Item>
	{:else if action === 'move'}
		<DropdownMenu.Item onSelect={() => ops.open({ kind: 'transfer', op: 'move', target })}>
			{m.shares_move_menu()}
		</DropdownMenu.Item>
	{:else if action === 'copy'}
		<DropdownMenu.Item onSelect={() => ops.open({ kind: 'transfer', op: 'copy', target })}>
			{m.shares_copy_menu()}
		</DropdownMenu.Item>
	{:else if action === 'download'}
		<!-- 行を押すとビューアーで開くものも、ファイルとして手元に置けるようにする。API への直リンク (→ $lib/api/urls.ts)。 -->
		<DropdownMenu.Item>
			{#snippet child({ props })}
				<a
					href={shareDownloadHref(dir.shareId, target.path)}
					download={target.name}
					rel="external"
					{...props}>{m.shares_download_menu()}</a
				>
			{/snippet}
		</DropdownMenu.Item>
	{:else if action === 'listOnHome'}
		<DropdownMenu.Item onSelect={() => ops.open({ kind: 'listOnHome', target })}>
			{m.shares_list_on_home_menu()}
		</DropdownMenu.Item>
	{:else if action === 'delete'}
		<DropdownMenu.Item variant="destructive" onSelect={() => ops.open({ kind: 'delete', target })}>
			{m.action_delete()}
		</DropdownMenu.Item>
	{/if}
{/each}
