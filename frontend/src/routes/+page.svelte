<script lang="ts">
	import BrowseControls from '#lib/components/browse-controls.svelte';
	import { ListFilterState, provideListFilter } from '#lib/list-filter.svelte.ts';
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import * as m from '#lib/paraglide/messages.js';
	import { SORT_OPTIONS, browseSortHref, type BrowseSort } from '#lib/browse-sort.ts';
	import BrowseSortSelect from '#lib/components/browse-sort.svelte';
	import ContentAddMenu from '#lib/components/content-add-menu.svelte';
	import ContentList from '#lib/components/content-list.svelte';
	import ShareList from '#lib/components/share-list.svelte';
	import type { PageProps } from './$types';
	import { pageTitle } from '#lib/page-title.ts';
	import { browseGutterClass } from '#lib/list-row.ts';
	import { pageEmptyTextClass } from '#lib/page-layout.ts';

	let { data }: PageProps = $props();

	// ページ内の絞り込み (→ docs/ui.md「一覧の絞り込み」)。一覧の部品が context から読む。
	const listFilter = new ListFilterState();
	provideListFilter(listFilter);

	// コンテンツはログイン済みなら `role` を問わず作れる (→ docs/access.md「ロールと操作」)。
	// 追加の入り口は、追加できる人にだけ出す。
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
	     読み上げ用の h1 だけを置く (→ docs/ui.md「UI 全般」)。追加できる人には、見出しの右に「追加」を置く。 -->
	{#if siteName !== '' || homeHeading !== '' || canAddContents}
		<div class={['mb-5 flex items-start gap-2', browseGutterClass]}>
			<div class="min-w-0 flex-1">
				{#if siteName !== ''}
					<p class="text-sm tracking-wider text-muted-foreground">{siteName}</p>
				{/if}
				{#if homeHeading !== ''}
					<h1 class={['text-2xl font-bold sm:text-3xl', siteName !== '' && 'mt-1.5']}>
						{homeHeading}
					</h1>
				{/if}
			</div>
			{#if canAddContents}
				<ContentAddMenu parentId={null} />
			{/if}
		</div>
	{/if}
	{#if homeHeading === ''}
		<h1 class="sr-only">{m.breadcrumb_home()}</h1>
	{/if}
	{#if data.contents.length === 0}
		<p class={pageEmptyTextClass}>{m.home_empty()}</p>
	{:else}
		<BrowseControls class={['mb-4', browseGutterClass]}>
			{#snippet sort()}
				<BrowseSortSelect value={data.sort} options={SORT_OPTIONS} href={sortHref} />
			{/snippet}
		</BrowseControls>
		<ContentList entries={data.contents} actions />
	{/if}
	{#if data.shares.length > 0}
		<!-- 共有はコンテンツとは別の区画にして、一覧の下に置く。ホームの主役はコンテンツのため。 -->
		<section class="mt-12" aria-labelledby="home-shares-heading">
			<h2 id="home-shares-heading" class={['mb-2 text-base font-semibold', browseGutterClass]}>
				{m.shares_heading()}
			</h2>
			<ShareList shares={data.shares} />
		</section>
	{/if}
</div>
