<script lang="ts" module>
	import { pickerCrumbButtonClass, pickerNavClass, pickerRowClass } from '#lib/dir-picker.ts';
	import type { components } from '#lib/api/schema.js';

	type ShareAccess = components['schemas']['ShareAccess'];

	/** 選んでいる置き先。共有の一覧を見ている間は `null`。 */
	export type PickedDir = { shareId: number; shareName: string; path: string; access: ShareAccess };
</script>

<script lang="ts">
	/**
	 * 移動・コピーの置き先を選ぶ一覧 (→ docs/ui.md「共有の中の画面」)。共有の一覧から始めて、フォルダーを辿る。
	 * 並べる共有は、見ている人が足せるものだけ。辿った先で足せないときは、呼び出し側が確定を押せなくする。
	 * `disabledPath` (動かす・写すフォルダー自身) には入れない。それ自身の中へは置けないため。
	 */
	import { untrack } from 'svelte';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import HardDriveIcon from '@lucide/svelte/icons/hard-drive';
	import { client } from '#lib/api/client.ts';
	import { GENERIC_ERROR_MESSAGE, errorMessage } from '#lib/api/errors.ts';
	import { pathCrumbs } from '#lib/breadcrumb.ts';
	import { LatestRequest } from '#lib/latest-request.ts';
	import * as m from '#lib/paraglide/messages.js';
	import { accessAtLeast } from '#lib/share-ops.ts';
	import { Spinner } from '#lib/components/ui/spinner/index.ts';

	type ShareResponse = components['schemas']['ShareResponse'];
	type DirsResponse = components['schemas']['ShareDirsResponse'];

	let {
		shares,
		start,
		disabledPath,
		onpick
	}: {
		/** 見ている人が使える共有。 */
		shares: ShareResponse[];
		/** 開いたときの場所。足せない場所なら共有の一覧から始める。 */
		start: { shareId: number; path: string } | null;
		/** 入れないフォルダー (動かす・写すフォルダー自身)。 */
		disabledPath: { shareId: number; path: string } | null;
		/** 見ている場所が変わるたびに呼ぶ。共有の一覧を見ている間は `null`。 */
		onpick: (picked: PickedDir | null) => void;
	} = $props();

	let listing = $state<DirsResponse | null>(null);
	let loading = $state(false);
	let loadError = $state('');
	const requests = new LatestRequest();

	let addable = $derived(shares.filter((share) => accessAtLeast(share.access, 'add')));
	let crumbs = $derived(listing === null ? [] : pathCrumbs(listing.path));

	async function load(shareId: number, path: string) {
		const isCurrent = requests.begin();
		loading = true;
		loadError = '';
		try {
			const { data, error, response } = await client.GET('/api/v1/shares/{id}/dirs', {
				params: { path: { id: shareId }, query: { path } }
			});
			if (!isCurrent()) return;
			if (!response.ok || !data) {
				loadError = errorMessage(error);
				return;
			}
			listing = data;
			onpick({
				shareId: data.share.id,
				shareName: data.share.name,
				path: data.path,
				access: data.access
			});
		} catch {
			if (isCurrent()) loadError = GENERIC_ERROR_MESSAGE();
		} finally {
			if (isCurrent()) loading = false;
		}
	}

	function showShares() {
		requests.invalidate();
		loading = false;
		loadError = '';
		listing = null;
		onpick(null);
	}

	$effect(() => {
		const initial = start;
		untrack(() => {
			if (initial === null) showShares();
			else void load(initial.shareId, initial.path);
		});
	});

	function isDisabled(shareId: number, path: string): boolean {
		return disabledPath !== null && disabledPath.shareId === shareId && disabledPath.path === path;
	}
</script>

<!-- パンくずは実際の高さを 44px にする (→ docs/ui.md「UI 全般」)。 -->
<nav aria-label={m.dir_picker_breadcrumb_label()} class={pickerNavClass}>
	{#if listing === null}
		<span class="text-xs text-muted-foreground">{m.shares_heading()}</span>
	{:else}
		{@const share = listing.share}
		<button type="button" class={pickerCrumbButtonClass} onclick={showShares}
			>{m.shares_heading()}</button
		>
		<span aria-hidden="true" class="text-muted-foreground">›</span>
		{#if crumbs.length === 0}
			<span class="text-xs text-muted-foreground">{share.name}</span>
		{:else}
			<button type="button" class={pickerCrumbButtonClass} onclick={() => load(share.id, '')}
				>{share.name}</button
			>
		{/if}
		{#each crumbs as crumb, index (crumb.path)}
			<span aria-hidden="true" class="text-muted-foreground">›</span>
			{#if index === crumbs.length - 1}
				<span class="text-xs wrap-anywhere text-muted-foreground">{crumb.name}</span>
			{:else}
				<button
					type="button"
					class={pickerCrumbButtonClass}
					onclick={() => load(share.id, crumb.path)}>{crumb.name}</button
				>
			{/if}
		{/each}
	{/if}
</nav>

<div class="min-h-48 flex-1 overflow-y-auto">
	{#if loadError !== ''}
		<p class="px-4 py-4 text-sm text-destructive">{loadError}</p>
	{:else if loading && listing === null}
		<div class="px-4 py-4"><Spinner aria-label={m.common_loading()} /></div>
	{:else if listing === null}
		{#each addable as share (share.id)}
			<button type="button" class={pickerRowClass} onclick={() => load(share.id, '')}>
				<HardDriveIcon class="size-5 shrink-0 text-sub-foreground" />
				<span class="flex-1 truncate text-sm">{share.name}</span>
				<ChevronRightIcon class="size-4 shrink-0 text-muted-foreground" />
			</button>
		{/each}
		{#if addable.length === 0}
			<p class="px-4 py-4 text-sm text-muted-foreground">{m.shares_picker_no_shares()}</p>
		{/if}
	{:else}
		{@const current = listing}
		<button
			type="button"
			class={pickerRowClass}
			onclick={() =>
				current.parent == null ? showShares() : load(current.share.id, current.parent)}
		>
			<ChevronLeftIcon class="size-5 shrink-0 text-muted-foreground" />
			<span class="flex-1 text-sm text-muted-foreground">{m.dir_picker_up()}</span>
		</button>
		{#each current.entries as entry (entry.path)}
			{@const disabled = isDisabled(current.share.id, entry.path)}
			<button
				type="button"
				class={pickerRowClass}
				{disabled}
				onclick={() => load(current.share.id, entry.path)}
			>
				<FolderIcon class="size-5 shrink-0 text-sub-foreground" />
				<span class="flex-1 truncate text-sm">{entry.name}</span>
				{#if !disabled}
					<ChevronRightIcon class="size-4 shrink-0 text-muted-foreground" />
				{/if}
			</button>
		{/each}
		{#if current.entries.length === 0}
			<p class="px-4 py-4 text-sm text-muted-foreground">{m.dir_picker_empty()}</p>
		{/if}
	{/if}
</div>
