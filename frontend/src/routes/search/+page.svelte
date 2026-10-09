<script lang="ts">
	import { onMount, untrack } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { withQuery } from '$lib/href';
	import { linksFileHref } from '$lib/links-file';
	import * as m from '$lib/paraglide/messages.js';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import ArchiveViewList, { type ArchiveViewRow } from '$lib/components/archive-view-list.svelte';
	import BrowseLayoutToggle from '$lib/components/browse-layout-toggle.svelte';
	import ContentList from '$lib/components/content-list.svelte';
	import FolderEntryList, { type FolderEntryRow } from '$lib/components/folder-entry-list.svelte';
	import LinksEntryList, { type LinksEntryRow } from '$lib/components/links-entry-list.svelte';
	import type { components } from '$lib/api/schema';
	import { browseControlsClass, browseGutterClass } from '$lib/list-row';
	import { pageEmptyTextClass, pageHeadingClass } from '$lib/page-layout';
	import { pageTitle } from '$lib/page-title';
	import { formatNumber } from '$lib/format';
	import type { PageProps } from './$types';

	type ContentEntry = components['schemas']['ContentResponse'];

	/** 打ち止めてから探すまでの間。1文字ごとに問い合わせず、打っている間に結果が揺れないようにする。 */
	const SEARCH_DELAY_MS = 300;

	let { data }: PageProps = $props();

	// 開いたときの語。以後は入力欄が持ち、URL はそれを追いかける。
	let query = $state(page.url.searchParams.get('q') ?? '');
	let input = $state<HTMLInputElement | null>(null);
	/** 最後に URL へ送った語。これと違う `q` が届いたら、入力欄の外 (ヘッダーの虫眼鏡など) で変わったもの。 */
	let sent = (page.url.searchParams.get('q') ?? '').trim();

	// 結果から戻ったときにキーボードを開いて結果を覆わないよう、語が空のときだけ欄にフォーカスを入れる。
	onMount(() => {
		if (query === '') input?.focus();
	});

	$effect.pre(() => {
		const q = data.q;
		untrack(() => {
			if (q.trim() === sent) return;
			query = q;
			sent = q.trim();
		});
	});

	function searchHref(q: string): string {
		return withQuery(resolve('/search'), q === '' ? {} : { q });
	}

	// 履歴は置き換えにして、打つたびに戻るの段を増やさない。戻ると、検索を開く前の画面へ戻る。
	function search(q: string) {
		sent = q;
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」)
		void goto(searchHref(q), { replaceState: true, keepFocus: true, noScroll: true });
	}

	$effect(() => {
		const next = query.trim();
		if (next === data.q.trim()) return;
		const timer = setTimeout(() => search(next), SEARCH_DELAY_MS);
		return () => clearTimeout(timer);
	});

	function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		const next = query.trim();
		if (next !== data.q.trim()) search(next);
	}

	let result = $derived(data.result);
	let contentHits = $derived(result?.contents ?? []);
	let itemHits = $derived(result?.items ?? []);
	let fileHits = $derived(result?.files ?? []);
	let linkHits = $derived(result?.links ?? []);
	let hasHits = $derived(
		contentHits.length + itemHits.length + fileHits.length + linkHits.length > 0
	);

	let placements = $derived(new Map(contentHits.map((hit) => [hit.content.id, hit])));

	/** どこにあるか。説明だけで当たったものは、何で当たったか分かるよう説明を添える。 */
	function contentSubtitle(content: ContentEntry): string[] {
		const hit = placements.get(content.id);
		const location = hit?.parent?.title ?? m.breadcrumb_home();
		return hit?.matchedInDescription && content.description
			? [location, content.description]
			: [location];
	}

	let itemRows = $derived<ArchiveViewRow[]>(
		itemHits.map((hit) => ({
			archiveId: hit.archiveId,
			item: hit.item,
			subtitle: [hit.archiveTitle, hit.item.subtitle].filter((part): part is string => !!part)
		}))
	);

	function linksHref(row: ArchiveViewRow): string {
		return linksFileHref(row.archiveId, { item: row.item.id });
	}

	/** フォルダの中の行の2段目。フォルダの名前と、その中のどこにあるか。 */
	let fileRows = $derived<FolderEntryRow[]>(
		fileHits.map((hit) => ({
			contentId: hit.contentId,
			path: hit.path,
			entry: hit.entry,
			subtitle: [hit.folderTitle, ...hit.path.split('/').slice(0, -1)].join(' / ')
		}))
	);

	function folderHref(row: FolderEntryRow): string {
		return withQuery(resolve('/folders/[id]', { id: String(row.contentId) }), { path: row.path });
	}

	function folderLinksHref(row: FolderEntryRow): string {
		return linksFileHref(row.contentId, { path: row.path });
	}

	/** 一覧のファイルの中のリンクの2段目。どの一覧にあるかと、`note` で当たったときは `note`。 */
	let linkRows = $derived<LinksEntryRow[]>(
		linkHits.map((hit) => {
			const location = [hit.containerTitle, hit.fileTitle]
				.filter((part): part is string => !!part)
				.join(' / ');
			return {
				contentId: hit.contentId,
				target: {
					...(hit.path != null ? { path: hit.path } : {}),
					...(hit.item != null ? { item: hit.item } : {})
				},
				url: hit.url,
				note: hit.note,
				preview: hit.preview,
				subtitle: hit.matchedInNote && hit.note ? [location, hit.note] : [location]
			};
		})
	);
</script>

<svelte:head><title>{pageTitle(m.search_title())}</title></svelte:head>

{#snippet sectionHeading(title: string, count: number, truncated: boolean)}
	<h2 class={['mb-2 flex items-baseline gap-2', browseGutterClass]}>
		<span class="text-base font-semibold">{title}</span>
		<span class="text-sm text-muted-foreground">
			{truncated
				? m.search_section_count_truncated({ count: formatNumber(count) })
				: m.search_section_count({ count: formatNumber(count) })}
		</span>
	</h2>
{/snippet}

{#snippet truncatedNote(count: number)}
	<p class={['mt-2', pageEmptyTextClass]}>
		{m.search_truncated({ count: formatNumber(count) })}
	</p>
{/snippet}

<div class="py-6">
	<div class={browseGutterClass}>
		<h1 class={pageHeadingClass}>{m.search_title()}</h1>

		<form role="search" onsubmit={handleSubmit}>
			<Field.Field>
				<Field.FieldLabel for="search-query">{m.search_query_label()}</Field.FieldLabel>
				<Input
					id="search-query"
					type="search"
					autocomplete="off"
					enterkeyhint="search"
					aria-describedby="search-query-help"
					bind:ref={input}
					bind:value={query}
				/>
				<Field.FieldDescription id="search-query-help">
					{m.search_query_description()}
				</Field.FieldDescription>
			</Field.Field>
		</form>

		{#if hasHits}
			<div class={['mt-6 mb-4 justify-end', browseControlsClass]}>
				<BrowseLayoutToggle />
			</div>
		{/if}
	</div>

	{#if data.failed}
		<p class={['mt-6', pageEmptyTextClass]}>{m.search_fetch_failed()}</p>
	{:else if result && !hasHits}
		<p class={['mt-6', pageEmptyTextClass]}>{m.search_empty({ query: data.q.trim() })}</p>
	{:else if result}
		{#if contentHits.length > 0}
			<section>
				{@render sectionHeading(
					m.search_contents_heading(),
					contentHits.length,
					result.contentsTruncated
				)}
				<ContentList entries={contentHits.map((hit) => hit.content)} subtitle={contentSubtitle} />
				{#if result.contentsTruncated}
					{@render truncatedNote(contentHits.length)}
				{/if}
			</section>
		{/if}
		{#if itemRows.length > 0}
			<section class={contentHits.length > 0 ? 'mt-8' : undefined}>
				{@render sectionHeading(m.search_items_heading(), itemRows.length, result.itemsTruncated)}
				<ArchiveViewList rows={itemRows} {linksHref} />
				{#if result.itemsTruncated}
					{@render truncatedNote(itemRows.length)}
				{/if}
			</section>
		{/if}
		{#if fileRows.length > 0}
			<section class={contentHits.length + itemRows.length > 0 ? 'mt-8' : undefined}>
				{@render sectionHeading(m.search_files_heading(), fileRows.length, result.filesTruncated)}
				<FolderEntryList rows={fileRows} dirHref={folderHref} linksHref={folderLinksHref} />
				{#if result.filesTruncated}
					{@render truncatedNote(fileRows.length)}
				{/if}
			</section>
		{/if}
		{#if linkRows.length > 0}
			<section
				class={contentHits.length + itemRows.length + fileRows.length > 0 ? 'mt-8' : undefined}
			>
				{@render sectionHeading(m.search_links_heading(), linkRows.length, result.linksTruncated)}
				<LinksEntryList rows={linkRows} />
				{#if result.linksTruncated}
					{@render truncatedNote(linkRows.length)}
				{/if}
			</section>
		{/if}
	{/if}
</div>
