<script lang="ts">
	import * as m from '#lib/paraglide/messages.js';
	import BrowseBreadcrumb from '#lib/components/browse-breadcrumb.svelte';
	import ShareList from '#lib/components/share-list.svelte';
	import { browseGutterClass } from '#lib/list-row.ts';
	import { pageEmptyTextClass, pageHeadingClass } from '#lib/page-layout.ts';
	import { pageTitle } from '#lib/page-title.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head><title>{pageTitle(m.shares_heading())}</title></svelte:head>

<div class="py-6">
	<div class={browseGutterClass}>
		<BrowseBreadcrumb ancestors={[]}>
			<span aria-hidden="true">/</span>
			<span>{m.shares_heading()}</span>
		</BrowseBreadcrumb>
		<h1 class={pageHeadingClass}>{m.shares_heading()}</h1>
	</div>

	{#if data.shares.length === 0}
		<p class={pageEmptyTextClass}>{m.shares_empty()}</p>
	{:else}
		<ShareList shares={data.shares} />
	{/if}
</div>
