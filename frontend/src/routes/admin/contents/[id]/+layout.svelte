<script lang="ts">
	import { pageHeadingInRowClass, sectionGridClass } from '$lib/page-layout';
	import { page } from '$app/state';
	import { isAdmin } from '$lib/auth';
	import { resolve } from '$app/paths';
	import { navTabClass, navTabListClass } from '$lib/nav-tabs';
	import { pageTitle } from '$lib/page-title';
	import { archiveItemsSummary, contentTypeLabel } from '$lib/content-labels';
	import { byIdMap } from '$lib/content-tree';
	import { contentViewHref } from '$lib/content-view-href';
	import { gotoWithoutLeaveConfirm } from '$lib/leave-confirm';
	import { Button } from '$lib/components/ui/button';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import ScanProgress from '$lib/components/scan-progress.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import { ArchiveRescanState, provideArchiveRescan } from '$lib/archive-rescan.svelte';
	import { cancelScan } from '$lib/archive-scan';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import ContentDeleteDialog from '$lib/components/content-delete-dialog.svelte';
	import RowActionsMenu from '$lib/components/row-actions-menu.svelte';
	import { contentTypeIcon } from '$lib/content-types';
	import { contentPathLabel } from '$lib/root-location';
	import * as m from '$lib/paraglide/messages.js';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import { backLinkClass } from '$lib/back-link';
	import EyeIcon from '@lucide/svelte/icons/eye';
	import type { LayoutProps } from './$types';

	let { data, children }: LayoutProps = $props();

	let content = $derived(data.content);
	let contentById = $derived(byIdMap(data.contents));
	let view = $derived(contentViewHref(content, page.data.user?.id));
	let deletingContent = $state<typeof content | null>(null);

	// 再スキャンは見出しの段で押し、進み具合と結果は見出しの下に出す (→ docs/ui.md「UI 全般」)。
	// タブを移っても消えないよう、ここで持つ。
	const rescan = new ArchiveRescanState();
	provideArchiveRescan(rescan);
	const errorDialog = new ErrorDialogState();
	let canRescan = $derived(data.editable && content.listView);
	let contentId = $derived(data.contentId);

	$effect(() => {
		const id = contentId;
		if (!canRescan) return;
		const controller = new AbortController();
		void rescan.watch(id, controller.signal);
		return () => {
			controller.abort();
			rescan.reset();
		};
	});

	// 一覧の表示を使うフォルダーは「基本情報 / 軸定義 / アイテム」を同列に並べる (→ docs/access.md「ロールと操作」)。
	// 対象外は、パターンを書き換える人のためのものなので、書き換えられる人にだけ出す。
	let tabs = $derived.by(() => {
		const id = String(data.contentId);
		const basic = {
			href: resolve('/admin/contents/[id]', { id }),
			label: m.contents_edit_tabs_basic()
		};
		if (!content.listView) return [basic];
		return [
			basic,
			{ href: resolve('/admin/contents/[id]/axes', { id }), label: m.archive_tabs_axes() },
			{ href: resolve('/admin/contents/[id]/items', { id }), label: m.archive_tabs_items() },
			...(data.editable
				? [
						{
							href: resolve('/admin/contents/[id]/excluded', { id }),
							label: m.archive_tabs_excluded()
						}
					]
				: [])
		];
	});

	let location = $derived(
		content.url ?? contentPathLabel(content, isAdmin(page.data.user)) ?? content.fileName
	);
	let TypeIcon = $derived(contentTypeIcon(content.type));
	// 見るだけの理由を、どのタブでも見出しの下で示す (→ docs/ui.md「UI 全般」)。
	function startRescan() {
		void rescan.run(data.contentId, (message) => errorDialog.show(message));
	}

	let viewOnlyNote = $derived.by(() => {
		if (data.editable) return null;
		const creator = content.createdByUsername;
		return creator
			? m.contents_edit_view_only({ creator })
			: m.contents_edit_view_only_no_creator();
	});
</script>

<svelte:head><title>{pageTitle(content.title)}</title></svelte:head>

<div class="px-4 pt-4 md:px-6">
	<a href={resolve('/admin/contents')} class={backLinkClass}>
		<ChevronLeftIcon class="size-4" />
		<span class="underline underline-offset-4">{m.admin_contents_nav_title()}</span>
	</a>
	<!-- 閲覧画面へのリンクと⋮は、どのタブでも同じ所に出すので見出しの段に置く (→ docs/ui.md「UI 全般」)。 -->
	<div class="mt-2 flex items-start gap-2">
		<h1 class={pageHeadingInRowClass}>{content.title}</h1>
		<div class="flex shrink-0 items-center gap-1">
			<!-- スマートフォンの幅では、タイトルに幅を残すため、どちらも⋮の中に入れる (→ docs/ui.md「UI 全般」)。 -->
			{#if canRescan}
				<LoadingButton
					class="max-sm:hidden"
					variant="outline"
					size="sm"
					loading={rescan.busy}
					onclick={startRescan}
				>
					<RefreshCwIcon class="size-4" />
					{m.archive_items_rescan_button()}
				</LoadingButton>
			{/if}
			<!-- eslint-disable svelte/no-navigation-without-resolve -- contentViewHref() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
			<Button
				class="max-sm:hidden"
				variant="outline"
				size="sm"
				href={view.href}
				target={view.external ? '_blank' : undefined}
				rel={view.external ? 'external noopener noreferrer' : undefined}
				>{m.contents_menu_view()}</Button
			>
			<!-- eslint-enable svelte/no-navigation-without-resolve -->
			<!-- 見るだけの人には消す操作が無いので、広い幅では⋮ごと出さない。 -->
			<div class={[!data.editable && 'sm:hidden']}>
				<RowActionsMenu name={content.title}>
					{#if canRescan}
						<DropdownMenu.Item class="sm:hidden" disabled={rescan.busy} onSelect={startRescan}>
							{m.archive_items_rescan_button()}
						</DropdownMenu.Item>
					{/if}
					<DropdownMenu.Item class="sm:hidden">
						{#snippet child({ props })}
							<!-- eslint-disable svelte/no-navigation-without-resolve -- contentViewHref() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
							<a
								href={view.href}
								target={view.external ? '_blank' : undefined}
								rel={view.external ? 'external noopener noreferrer' : undefined}
								{...props}>{m.contents_menu_view()}</a
							>
							<!-- eslint-enable svelte/no-navigation-without-resolve -->
						{/snippet}
					</DropdownMenu.Item>
					{#if data.editable}
						<DropdownMenu.Item variant="destructive" onSelect={() => (deletingContent = content)}>
							{m.action_delete()}
						</DropdownMenu.Item>
					{/if}
				</RowActionsMenu>
			</div>
		</div>
	</div>
	<!-- 種別は後から変えられないので、フォームではなくここに出す。
	     区切りの「・」は見た目だけのものなので読み上げない。 -->
	<p class="flex flex-wrap items-center gap-x-1 text-sm text-muted-foreground">
		<TypeIcon class="size-4 shrink-0" aria-hidden="true" />
		<span>{contentTypeLabel(content.type, content.listView)}</span>
		{#if location}
			<span aria-hidden="true">・</span>
			<!-- パスは区切り以外に折り返せる場所が無く、そのままだと画面からはみ出す
			     (スマートフォン幅)。どこでも折り返してよい扱いにする。 -->
			<span class="min-w-0 wrap-anywhere">{location}</span>
		{/if}
		{#if content.archiveItems}
			<span aria-hidden="true">・</span>
			<span>{archiveItemsSummary(content.archiveItems)}</span>
		{/if}
	</p>
	{#if viewOnlyNote}
		<!-- 下の区画の1列目と同じ幅にする。どの画面幅でも端が揃うよう、同じ格子に置く。 -->
		<div class={[sectionGridClass, 'mt-3']}>
			<p
				class="flex items-start gap-2 rounded-md border bg-muted/40 px-3 py-2 text-sm"
				data-testid="view-only-note"
			>
				<EyeIcon class="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
				<span>{viewOnlyNote}</span>
			</p>
		</div>
	{/if}
</div>

{#if rescan.progress || rescan.result}
	<!-- タブの上に出し、タブを移っても出したままにする。 -->
	<div class="mt-2 px-4 md:px-6">
		{#if rescan.progress}
			<ScanProgress message={rescan.progress} oncancel={() => cancelScan(data.contentId)} />
		{:else}
			<p class="text-sm text-muted-foreground">{rescan.result}</p>
		{/if}
	</div>
{/if}

<!-- 切り替え先が無いときはタブの列ごと出さない。 -->
{#if tabs.length > 1}
	<nav aria-label={m.contents_edit_tabs_label()} class={[navTabListClass, 'mt-2']}>
		{#each tabs as tab (tab.href)}
			{@const current = page.url.pathname === tab.href}
			<a href={tab.href} aria-current={current ? 'page' : undefined} class={navTabClass(current)}
				>{tab.label}</a
			>
		{/each}
	</nav>
{/if}

<!-- 別のコンテンツへ移ったら、フォームを読み込み直すためにページを作り直す。 -->
{#key data.contentId}
	{@render children()}
{/key}

<!-- 消したものの編集ページには戻れないので、一覧へ移る。 -->
<ContentDeleteDialog
	bind:content={deletingContent}
	contents={data.contents}
	{contentById}
	ondeleted={() => gotoWithoutLeaveConfirm(resolve('/admin/contents'))}
/>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
