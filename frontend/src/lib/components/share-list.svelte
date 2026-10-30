<script lang="ts">
	import HardDriveIcon from '@lucide/svelte/icons/hard-drive';
	import {
		browseItemClass,
		browseListClass,
		browseRowClass,
		browseRowPressClass,
		browseRowTextClass,
		browseRowTitleClass
	} from '$lib/list-row';
	import { resolve } from '$app/paths';
	import ListRowIcon from '$lib/components/list-row-icon.svelte';
	import ListRowSubtitle from '$lib/components/list-row-subtitle.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import type { components } from '$lib/api/schema';

	type Share = components['schemas']['ShareResponse'];
	type ShareAccess = components['schemas']['ShareAccess'];

	// ログインした人が使える共有の行。ホームの「共有」の区画と、共有の一覧 (`/shares`) で同じ行にする。
	// 2段目には、その人が共有の中でできることを出す (読むだけの共有で、追加の口が無い理由が分かるように)。
	let { shares }: { shares: Share[] } = $props();

	function accessLabel(access: ShareAccess): string | undefined {
		if (access === 'read') return m.shares_access_read();
		if (access === 'add') return m.shares_access_add();
		if (access === 'all') return m.shares_access_all();
		// `none` は一覧に来ない。
		return undefined;
	}
</script>

<ul class={browseListClass()}>
	{#each shares as share (share.id)}
		<li class={browseItemClass()}>
			<div class={browseRowClass()}>
				<ListRowIcon icon={HardDriveIcon} />
				<span class={browseRowTextClass()}>
					<a href={resolve('/shares/[id]', { id: String(share.id) })} class={browseRowPressClass()}>
						<span class={browseRowTitleClass()}>{share.name}</span>
					</a>
					<ListRowSubtitle text={accessLabel(share.access)} />
				</span>
			</div>
		</li>
	{/each}
</ul>
