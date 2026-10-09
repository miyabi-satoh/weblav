<script lang="ts">
	import BrowseFilterInput from '$lib/components/browse-filter-input.svelte';
	import { ListFilterState, provideListFilter } from '$lib/list-filter.svelte';
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import * as m from '$lib/paraglide/messages.js';
	import { SORT_OPTIONS, browseSortHref, type BrowseSort } from '$lib/browse-sort';
	import BrowseLayoutToggle from '$lib/components/browse-layout-toggle.svelte';
	import BrowseSortSelect from '$lib/components/browse-sort.svelte';
	import ContentList from '$lib/components/content-list.svelte';
	import type { PageProps } from './$types';
	import { pageTitle } from '$lib/page-title';
	import { browseControlsClass, browseGutterClass } from '$lib/list-row';
	import { pageEmptyTextClass } from '$lib/page-layout';

	let { data }: PageProps = $props();

	// ページ内の絞り込み (→ docs/ui.md「一覧の絞り込み」)。一覧の部品が context から読む。
	const listFilter = new ListFilterState();
	provideListFilter(listFilter);

	// コンテンツ管理はログイン済みなら `role` を問わず開ける (→ docs/access.md「管理画面の一覧が `user` に見えること」)。
	// 自分で追加できる人に「管理者に追加を依頼してください」と出さない。
	let canAddContents = $derived(page.data.user != null);

	let siteName = $derived(data.siteSettings.siteName);
	let homeHeading = $derived(data.siteSettings.homeHeading);

	function sortHref(sort: BrowseSort): string {
		// 並び順を変えても、打った絞り込みは残す。
		return browseSortHref(resolve('/'), sort, listFilter.query);
	}
</script>

<svelte:head><title>{pageTitle()}</title></svelte:head>

<div class="py-6">
	<!-- サイト名と見出しは管理画面で設定する。未設定の行は出さず、見出しが無ければ
	     読み上げ用の h1 だけを置く (→ docs/ui.md「UI 全般」)。 -->
	{#if siteName !== '' || homeHeading !== ''}
		<div class={['mb-5', browseGutterClass]}>
			{#if siteName !== ''}
				<p class="text-sm tracking-wider text-muted-foreground">{siteName}</p>
			{/if}
			{#if homeHeading !== ''}
				<h1 class={['text-2xl font-bold sm:text-3xl', siteName !== '' && 'mt-1.5']}>
					{homeHeading}
				</h1>
			{/if}
		</div>
	{/if}
	{#if homeHeading === ''}
		<h1 class="sr-only">{m.breadcrumb_home()}</h1>
	{/if}
	{#if data.contents.length === 0}
		{#if canAddContents}
			<div class={browseGutterClass}>
				<p class="text-sm text-muted-foreground">{m.home_empty_can_add()}</p>
				<a
					href={resolve('/admin/contents')}
					class="relative mt-1 inline-flex h-11 items-center text-sm text-primary underline underline-offset-4"
					>{m.home_empty_add_link()}</a
				>
			</div>
		{:else}
			<p class={pageEmptyTextClass}>{m.home_empty()}</p>
		{/if}
	{:else}
		<div class={['mb-4', browseControlsClass, browseGutterClass]}>
			<div class="flex flex-wrap gap-2.5">
				<BrowseSortSelect value={data.sort} options={SORT_OPTIONS} href={sortHref} />
				<BrowseFilterInput />
			</div>
			<BrowseLayoutToggle />
		</div>
		<ContentList entries={data.contents} />
	{/if}
</div>
