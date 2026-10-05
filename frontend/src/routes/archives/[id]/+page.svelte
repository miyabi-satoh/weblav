<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { withQuery } from '$lib/href';
	import * as m from '$lib/paraglide/messages.js';
	import { archiveItemDownloadHref, archiveItemThumbnailHref } from '$lib/api/urls';
	import { isAudioFileName, isLinksFileName, viewerFileKind } from '$lib/file-kind';
	import { linksFileHref, LinksFileIcon } from '$lib/links-file';
	import type { ViewerFile } from '$lib/file-viewer.svelte';
	import type { Track } from '$lib/now-playing.svelte';
	import type { ViewerImage } from '$lib/image-viewer';
	import { ARCHIVE_SORT_OPTIONS, archiveSortHref, type ArchiveSort } from '$lib/archive-sort';
	import {
		browseControlsClass,
		browseGutterClass,
		browseItemClass,
		browseListClass,
		browseRowClass,
		browseRowSubtitleClass,
		browseRowTextClass,
		browseRowTitleClass
	} from '$lib/list-row';
	import * as Select from '$lib/components/ui/select';
	import BrowseBreadcrumb from '$lib/components/browse-breadcrumb.svelte';
	import BrowseSelectTrigger from '$lib/components/browse-select-trigger.svelte';
	import BrowseLayoutToggle from '$lib/components/browse-layout-toggle.svelte';
	import BrowseSortSelect from '$lib/components/browse-sort.svelte';
	import ListRowGlyph from '$lib/components/list-row-glyph.svelte';
	import ListRowIcon from '$lib/components/list-row-icon.svelte';
	import ListRowPlayButton from '$lib/components/list-row-play-button.svelte';
	import ListRowImageLink from '$lib/components/list-row-image-link.svelte';
	import ListRowFileLink from '$lib/components/list-row-file-link.svelte';
	import FileIcon from '@lucide/svelte/icons/file';
	import type { components } from '$lib/api/schema';
	import type { PageProps } from './$types';
	import { pageTitle } from '$lib/page-title';
	import { pageHeadingClass, pageEmptyTextClass } from '$lib/page-layout';
	import { formatNumber } from '$lib/format';

	type ArchiveViewItem = components['schemas']['ArchiveViewItem'];
	type ArchiveAxis = components['schemas']['ArchiveAxisResponse'];

	let { data }: PageProps = $props();

	let contentId = $derived(data.contentId);
	let view = $derived(data.view);

	/**
	 * クエリで選ばれている選択肢。どの選択肢にも一致しない値はサーバーが無視するので
	 * (→ docs/archive.md「エンドポイント一覧」)、画面も絞り込み中として扱わない。
	 */
	function selectedOption(axis: ArchiveAxis) {
		const current = data.filters[axis.name];
		return axis.options.find((option) => option.value === current);
	}

	let hasFilters = $derived(view.axes.some((axis) => selectedOption(axis) !== undefined));

	// 「すべて」用のセンチネル値。軸の値(管理者が付けた任意の表示名)と
	// 衝突しないよう、その軸の実際のoptionsに存在しない文字列になるまで接頭辞を足す。
	function allValueFor(axis: ArchiveAxis): string {
		let candidate = '__all__';
		while (axis.options.some((option) => option.value === candidate)) {
			candidate = `_${candidate}`;
		}
		return candidate;
	}

	function axisValue(axis: ArchiveAxis): string {
		return selectedOption(axis)?.value ?? allValueFor(axis);
	}

	function axisLabel(axis: ArchiveAxis): string {
		return selectedOption(axis)?.value ?? m.archive_view_filter_all();
	}

	function filterHref(axis: ArchiveAxis, value: string): string {
		const nextFilters: Record<string, string> = { ...data.filters };
		if (value === allValueFor(axis)) {
			delete nextFilters[axis.name];
		} else {
			nextFilters[axis.name] = value;
		}
		return withQuery(resolve('/archives/[id]', { id: String(contentId) }), nextFilters);
	}

	function handleAxisChange(axis: ArchiveAxis, value: string) {
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」)
		goto(filterHref(axis, value));
	}

	function sortHref(sort: ArchiveSort): string {
		return archiveSortHref(
			resolve('/archives/[id]', { id: String(contentId) }),
			data.filters,
			sort
		);
	}

	function toTrack(item: ArchiveViewItem): Track {
		return { src: archiveItemDownloadHref(contentId, item.id), title: item.title };
	}

	let audioQueue = $derived(
		view.items.filter((item) => isAudioFileName(item.fileName)).map(toTrack)
	);

	/** 画像の行なら、ビューアに渡す形。大きさが分からない (読めない) 画像は普通のファイルの行にする。 */
	function toViewerImage(item: ArchiveViewItem): ViewerImage | undefined {
		if (!item.image) return undefined;
		return {
			src: archiveItemDownloadHref(contentId, item.id),
			thumbnailSrc: archiveItemThumbnailHref(contentId, item.id),
			width: item.image.width,
			height: item.image.height,
			title: item.title
		};
	}

	let viewerImages = $derived(view.items.map(toViewerImage).filter((image) => image !== undefined));

	/** 画像でない行に縮小画像を出すなら、その URL (→ docs/ui.md「画像のプレビュー」)。 */
	function rowThumbnail(item: ArchiveViewItem): { src: string; original: string } | undefined {
		if (!item.thumbnail) return undefined;
		return {
			src: archiveItemThumbnailHref(contentId, item.id),
			original: archiveItemDownloadHref(contentId, item.id)
		};
	}

	function linksHref(item: ArchiveViewItem): string {
		return linksFileHref(contentId, { item: item.id }, data.filters);
	}

	/** PDF・動画・テキストなど、ビューアで開く行なら、ビューアに渡す形 (→ docs/ui.md「PDF・動画・テキストのビューア」)。 */
	function toViewerFile(item: ArchiveViewItem): ViewerFile | undefined {
		const kind = viewerFileKind(item.fileName, item.isText);
		if (!kind) return undefined;
		return {
			src: archiveItemDownloadHref(contentId, item.id),
			title: item.title,
			fileName: item.fileName,
			kind,
			thumbnail: rowThumbnail(item)
		};
	}

	let viewerFiles = $derived(view.items.map(toViewerFile).filter((file) => file !== undefined));

	let resetHref = $derived(resolve('/archives/[id]', { id: String(contentId) }));
</script>

<svelte:head><title>{pageTitle(view.archiveTitle)}</title></svelte:head>

<!-- 2段目は、同じタイトルが並んだときに見分けるための軸の値 (→ docs/ui.md「アーカイブの一覧画面」)。 -->
{#snippet itemText(item: ArchiveViewItem)}
	<span class={browseRowTextClass()}>
		<span class={browseRowTitleClass()}>{item.title}</span>
		{#if item.subtitle}
			<span class={browseRowSubtitleClass()}>{item.subtitle}</span>
		{/if}
	</span>
{/snippet}

<div class="py-6">
	<div class={browseGutterClass}>
		<BrowseBreadcrumb ancestors={view.ancestors}>
			<span aria-hidden="true">/</span>
			<span>{view.archiveTitle}</span>
		</BrowseBreadcrumb>

		<h1 class={pageHeadingClass}>{view.archiveTitle}</h1>

		{#if view.items.length > 0 || view.axes.length > 0}
			<div class={browseControlsClass}>
				<div class="flex flex-wrap gap-2.5">
					<BrowseSortSelect value={data.sort} options={ARCHIVE_SORT_OPTIONS} href={sortHref} />
					{#each view.axes as axis (axis.name)}
						{@const filtered = selectedOption(axis) !== undefined}
						<Select.Root
							type="single"
							bind:value={() => axisValue(axis), (v) => handleAxisChange(axis, v)}
						>
							<BrowseSelectTrigger
								label={axis.name}
								value={axisLabel(axis)}
								highlighted={filtered}
							/>
							<Select.Content>
								<Select.Item value={allValueFor(axis)}>{m.archive_view_filter_all()}</Select.Item>
								<!-- 選んだときに残る件数を添える。0件の組み合わせを選ぶ前に見分けられる
								     ようにするためで、選択肢自体は 0 でも消さない (→ docs/archive.md「エンドポイント一覧」)。 -->
								{#each axis.options as option (option.value)}
									<Select.Item value={option.value}>
										<span>{option.value}</span>
										<span class="text-xs text-muted-foreground">{option.count}</span>
									</Select.Item>
								{/each}
							</Select.Content>
						</Select.Root>
					{/each}
				</div>
				<!-- 絞り込みで0件になっても並び順と絞り込みは残すが、並べる物が無いので切り替えは出さない。 -->
				{#if view.items.length > 0}
					<BrowseLayoutToggle />
				{/if}
			</div>
		{/if}

		<div class="mt-2 flex min-h-11 items-center justify-between gap-3 text-sm">
			<p class="text-muted-foreground">
				{m.archive_view_count({ count: formatNumber(view.items.length) })}
			</p>
			{#if hasFilters}
				<!-- パンくずと同じく実URL遷移にする。ブラウザの戻る/進むを機能させるため。 -->
				<a
					href={resetHref}
					class="inline-flex h-11 items-center px-1 text-primary underline underline-offset-4"
				>
					{m.archive_view_reset_filters()}
				</a>
			{/if}
		</div>
	</div>

	{#if view.items.length === 0}
		<p class={pageEmptyTextClass}>{m.archive_view_empty()}</p>
	{:else}
		<ul class={browseListClass()}>
			{#each view.items as item (item.id)}
				{@const image = toViewerImage(item)}
				{@const viewerFile = toViewerFile(item)}
				<li class={browseItemClass()}>
					{#if isLinksFileName(item.fileName)}
						<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
						<a href={linksHref(item)} class={browseRowClass()}>
							<ListRowIcon icon={LinksFileIcon} />
							{@render itemText(item)}
							<ListRowGlyph />
						</a>
					{:else if isAudioFileName(item.fileName)}
						<ListRowPlayButton track={toTrack(item)} queue={audioQueue}>
							{@render itemText(item)}
						</ListRowPlayButton>
					{:else if image}
						<ListRowImageLink {image} images={viewerImages}>
							{@render itemText(item)}
						</ListRowImageLink>
					{:else if viewerFile}
						<ListRowFileLink file={viewerFile} files={viewerFiles}>
							{@render itemText(item)}
						</ListRowFileLink>
					{:else}
						<!-- API への直リンク (→ $lib/api/urls.ts)。 -->
						<a
							href={archiveItemDownloadHref(contentId, item.id)}
							class={browseRowClass()}
							target="_blank"
							rel="external noopener noreferrer"
						>
							<ListRowIcon icon={FileIcon} thumbnail={rowThumbnail(item)} />
							{@render itemText(item)}
							<ListRowGlyph newTab />
						</a>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}
</div>
