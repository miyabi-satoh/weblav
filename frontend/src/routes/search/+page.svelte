<script lang="ts">
	import { onMount, untrack } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import HouseIcon from '@lucide/svelte/icons/house';
	import { contentTypeIcon } from '$lib/content-types';
	import { withQuery } from '$lib/href';
	import { linksFileHref, LinksFileIcon } from '$lib/links-file';
	import type { RowLocation, RowLocationLevel } from '$lib/row-location';
	import * as m from '$lib/paraglide/messages.js';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import ArchiveViewList, { type ArchiveViewRow } from '$lib/components/archive-view-list.svelte';
	import BrowseLayoutToggle from '$lib/components/browse-layout-toggle.svelte';
	import ContentList from '$lib/components/content-list.svelte';
	import FolderEntryList, { type FolderEntryRow } from '$lib/components/folder-entry-list.svelte';
	import LinksEntryList, { type LinksEntryRow } from '$lib/components/links-entry-list.svelte';
	import type { components } from '$lib/api/schema';
	import { browseControlsClass, browseGutterClass, browseToggleItemClass } from '$lib/list-row';
	import { pageEmptyTextClass, pageHeadingClass } from '$lib/page-layout';
	import { pageTitle } from '$lib/page-title';
	import { searchHref, searchScopeLevels } from '$lib/search-scope';
	import { formatNumber } from '$lib/format';
	import type { PageProps } from './$types';

	type ContentEntry = components['schemas']['ContentResponse'];

	// 範囲の名前は、幅が足りなければフォルダーの名前だけを切り詰める。訳の語順を崩さないよう、
	// 文言を目印で組んでから、名前の前と後ろに分ける。
	const SCOPE_TITLE_MARK = '\u0000';
	let scopeLabelParts = $derived(
		m.search_scope_within({ title: SCOPE_TITLE_MARK }).split(SCOPE_TITLE_MARK)
	);

	/** 打ち止めてから探すまでの間。1文字ごとに問い合わせず、打っている間に結果が揺れないようにする。 */
	const SEARCH_DELAY_MS = 300;
	/** 打ち止め待ちの検索。範囲を切り替えたら取り消す。残すと、切り替える前の範囲で URL を上書きするため。 */
	let pendingSearch: ReturnType<typeof setTimeout> | undefined;

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

	// 履歴は置き換えにして、打つたびに戻るの段を増やさない。戻ると、検索を開く前の画面へ戻る。
	function search(q: string) {
		sent = q;
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- searchHref() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」)
		void goto(searchHref(q, data.scope, data.all), {
			replaceState: true,
			keepFocus: true,
			noScroll: true
		});
	}

	/** 範囲の切り替え。語はまだ URL に送っていない打ちかけの分も含めて、そのまま探し直す。 */
	function switchScope(all: boolean) {
		const q = query.trim();
		sent = q;
		clearTimeout(pendingSearch);
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- searchHref() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」)
		void goto(searchHref(q, data.scope, all), {
			replaceState: true,
			keepFocus: true,
			noScroll: true
		});
	}

	$effect(() => {
		const next = query.trim();
		if (next === data.q.trim()) return;
		pendingSearch = setTimeout(() => search(next), SEARCH_DELAY_MS);
		return () => clearTimeout(pendingSearch);
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

	// 2段目に出す、どこにあるか。押すと階層を並べ、選んだ段の画面を開く (→ docs/search.md「画面」)。

	let placements = $derived(new Map(contentHits.map((hit) => [hit.content.id, hit])));

	/** 上のグループ。ルート直下なら「ホーム」。 */
	function contentLocation(content: ContentEntry): RowLocation {
		const ancestors = placements.get(content.id)?.ancestors ?? [];
		if (ancestors.length === 0) {
			return { levels: [{ label: m.breadcrumb_home(), href: resolve('/'), icon: HouseIcon }] };
		}
		return {
			levels: ancestors.map((group) => ({
				label: group.title,
				href: resolve('/groups/[id]', { id: String(group.id) }),
				icon: contentTypeIcon('group')
			}))
		};
	}

	/** 説明だけで当たったものは、何で当たったか分かるよう説明を添える。 */
	function contentSubtitle(content: ContentEntry): string | null {
		const hit = placements.get(content.id);
		return hit?.matchedInDescription && content.description ? content.description : null;
	}

	function archiveLevel(id: number, title: string): RowLocationLevel {
		return {
			label: title,
			href: resolve('/archives/[id]', { id: String(id) }),
			icon: contentTypeIcon('archive')
		};
	}

	/** フォルダーと、その中の `dirs` までの階層。 */
	function folderLevels(id: number, title: string, dirs: string[]): RowLocationLevel[] {
		const href = resolve('/folders/[id]', { id: String(id) });
		return [
			{ label: title, href, icon: contentTypeIcon('folder') },
			...dirs.map((label, index) => ({
				label,
				href: withQuery(href, { path: dirs.slice(0, index + 1).join('/') }),
				icon: FolderIcon
			}))
		];
	}

	let itemRows = $derived<ArchiveViewRow[]>(
		itemHits.map((hit) => ({
			archiveId: hit.archiveId,
			item: hit.item,
			location: { levels: [archiveLevel(hit.archiveId, hit.archiveTitle)] }
		}))
	);

	function linksHref(row: ArchiveViewRow): string {
		return linksFileHref(row.archiveId, { item: row.item.id });
	}

	/**
	 * 範囲にしているフォルダーの階層の深さ。その中を探している間は、範囲より上はどの行も同じなので、
	 * 行には範囲から下だけを出す。
	 */
	let scopeDepth = $derived(
		data.scope && !data.all
			? { contentId: Number(data.scope.within), depth: data.scope.path?.split('/').length ?? 0 }
			: null
	);

	/** フォルダーの中の `path` にあるものの場所。`path` は、そのもの自身の名前まで含む。 */
	function inFolderLocation(contentId: number, folderTitle: string, path: string): RowLocation {
		return {
			levels: folderLevels(contentId, folderTitle, path.split('/').slice(0, -1)),
			...(scopeDepth?.contentId === contentId ? { from: scopeDepth.depth } : {})
		};
	}

	let fileRows = $derived<FolderEntryRow[]>(
		fileHits.map((hit) => ({
			contentId: hit.contentId,
			path: hit.path,
			entry: hit.entry,
			location: inFolderLocation(hit.contentId, hit.folderTitle, hit.path)
		}))
	);

	function folderHref(row: FolderEntryRow): string {
		return withQuery(resolve('/folders/[id]', { id: String(row.contentId) }), { path: row.path });
	}

	function folderLinksHref(row: FolderEntryRow): string {
		return linksFileHref(row.contentId, { path: row.path });
	}

	/** リンクの一覧の中の行。どの一覧にあるかと、`note` で当たったときは `note` を2段目に出す。 */
	let linkRows = $derived<LinksEntryRow[]>(
		linkHits.map((hit) => {
			const target = {
				...(hit.path != null ? { path: hit.path } : {}),
				...(hit.item != null ? { item: hit.item } : {})
			};
			// 一覧のファイルを入れているフォルダーの階層かアーカイブ。`file` コンテンツの一覧には無い。
			const container: RowLocation =
				hit.containerTitle == null
					? { levels: [] }
					: hit.path != null
						? inFolderLocation(hit.contentId, hit.containerTitle, hit.path)
						: { levels: [archiveLevel(hit.contentId, hit.containerTitle)] };
			return {
				contentId: hit.contentId,
				target,
				url: hit.url,
				note: hit.note,
				preview: hit.preview,
				subtitle: hit.matchedInNote && hit.note ? hit.note : null,
				location: {
					...container,
					levels: [
						...container.levels,
						{
							label: hit.fileTitle,
							href: linksFileHref(hit.contentId, target),
							icon: LinksFileIcon
						}
					]
				}
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

		{#if data.scopeTitle !== null || hasHits}
			<!-- 並び順と同じく範囲は左に、リストとタイルの切り替えは右端に置く (→ docs/ui.md「UI 全般」)。 -->
			<!-- 範囲の名前が収まらなければ、切り替えを折り返して1段を使わせる。 -->
			<div class={['mt-6 mb-4 flex-wrap', browseControlsClass]}>
				{#if data.scopeTitle !== null}
					<!-- 閲覧ページから開いたときだけ出す。最初はそのページの中を選んでおく (→ docs/search.md「範囲」)。 -->
					<ToggleGroup.Root
						type="single"
						variant="outline"
						class="min-w-0"
						aria-label={m.search_scope_label()}
						bind:value={
							() => (data.all ? 'all' : 'within'),
							// 選んでいる側をもう一度押すと選択が外れる (空の値が来る) が、どちらかは常に選んでおく。
							(next) => {
								if (next === 'within' || next === 'all') switchScope(next === 'all');
							}
						}
					>
						<!-- 部品の既定の shrink-0 を外し、幅が足りなければこちらを縮める。 -->
						<ToggleGroup.Item value="within" class={['min-w-0 shrink px-4', browseToggleItemClass]}>
							{@const [before, after] = scopeLabelParts}
							{@const levels = searchScopeLevels(data.scope?.path)}
							<!-- 幅が足りなければフォルダーの名前 (頭の1字は残す) だけを縮める。階層は見分けるのに要るので縮めず、 -->
							<!-- スマートフォンの幅では、切り替えが1段に収まるよう上限を決めて、超えた分だけを切り詰める。 -->
							<span class="flex min-w-0">
								<span class="shrink-0">{before}</span>
								<span class="min-w-7 truncate">{data.scopeTitle}</span>
								<!-- 先頭の空白 (` / 2025`) が flex の項目の端で詰められないよう、空白をそのまま残す。 -->
								<span
									class="max-w-16 shrink-0 overflow-hidden text-ellipsis whitespace-pre sm:max-w-none"
									>{levels.upper}</span
								>
								<span
									class="max-w-24 shrink-0 overflow-hidden text-ellipsis whitespace-pre sm:max-w-none"
									>{levels.last}</span
								>
								<span class="shrink-0">{after}</span>
							</span>
						</ToggleGroup.Item>
						<ToggleGroup.Item value="all" class={['shrink-0 px-4', browseToggleItemClass]}>
							{m.search_scope_all()}
						</ToggleGroup.Item>
					</ToggleGroup.Root>
				{/if}
				{#if hasHits}
					<div class="ml-auto"><BrowseLayoutToggle /></div>
				{/if}
			</div>
		{/if}
	</div>

	{#if data.failed}
		<p class={['mt-6', pageEmptyTextClass]}>{m.search_fetch_failed()}</p>
	{:else if result && !hasHits}
		<p class={['mt-6', pageEmptyTextClass]}>{m.search_empty({ query: data.q.trim() })}</p>
		{#if result.foldersIncomplete}
			<p class={['mt-2', pageEmptyTextClass]}>{m.search_folders_incomplete()}</p>
		{/if}
	{:else if result}
		{#if contentHits.length > 0}
			<section>
				{@render sectionHeading(
					m.search_contents_heading(),
					contentHits.length,
					result.contentsTruncated
				)}
				<ContentList
					entries={contentHits.map((hit) => hit.content)}
					subtitle={contentSubtitle}
					location={contentLocation}
				/>
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
		{#if result.foldersIncomplete}
			<p class={['mt-8', pageEmptyTextClass]}>{m.search_folders_incomplete()}</p>
		{/if}
	{/if}
</div>
