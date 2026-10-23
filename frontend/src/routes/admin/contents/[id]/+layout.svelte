<script lang="ts">
	import { pageHeadingInRowClass } from '$lib/page-layout';
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
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import ContentDeleteDialog from '$lib/components/content-delete-dialog.svelte';
	import RowActionsMenu from '$lib/components/row-actions-menu.svelte';
	import { contentTypeIcon } from '$lib/content-types';
	import { contentPathLabel } from '$lib/root-location';
	import * as m from '$lib/paraglide/messages.js';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import EyeIcon from '@lucide/svelte/icons/eye';
	import type { LayoutProps } from './$types';

	let { data, children }: LayoutProps = $props();

	let content = $derived(data.content);
	let contentById = $derived(byIdMap(data.contents));
	let view = $derived(contentViewHref(content));
	let deletingContent = $state<typeof content | null>(null);

	// 一覧の表示を使うフォルダーは「基本情報 / 軸定義 / アイテム」を同列に並べる (→ docs/access.md「ロールと操作」)。
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
			{ href: resolve('/admin/contents/[id]/items', { id }), label: m.archive_tabs_items() }
		];
	});

	let location = $derived(
		content.url ?? contentPathLabel(content, isAdmin(page.data.user)) ?? content.fileName
	);
	let TypeIcon = $derived(contentTypeIcon(content.type));
	// 見るだけの理由を、どのタブでも見出しの下で示す (→ docs/ui.md「UI 全般」)。
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
	<a
		href={resolve('/admin/contents')}
		class="relative inline-flex items-center gap-1 text-sm text-muted-foreground after:absolute after:inset-x-0 after:-inset-y-3 hover:text-foreground"
	>
		<ChevronLeftIcon class="size-4" />
		<span class="underline underline-offset-4">{m.admin_contents_nav_title()}</span>
	</a>
	<!-- 閲覧画面へのリンクと⋮は、どのタブでも同じ所に出すので見出しの段に置く (→ docs/ui.md「UI 全般」)。 -->
	<div class="mt-2 flex items-start gap-2">
		<h1 class={pageHeadingInRowClass}>{content.title}</h1>
		<div class="flex shrink-0 items-center gap-1">
			<!-- eslint-disable svelte/no-navigation-without-resolve -- contentViewHref() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
			<Button
				variant="outline"
				size="sm"
				href={view.href}
				target={view.external ? '_blank' : undefined}
				rel={view.external ? 'external noopener noreferrer' : undefined}
				>{m.contents_menu_view()}</Button
			>
			<!-- eslint-enable svelte/no-navigation-without-resolve -->
			<!-- 見るだけの人には消す操作が無いので、⋮ごと出さない。 -->
			{#if data.editable}
				<RowActionsMenu name={content.title}>
					<DropdownMenu.Item variant="destructive" onSelect={() => (deletingContent = content)}>
						{m.action_delete()}
					</DropdownMenu.Item>
				</RowActionsMenu>
			{/if}
		</div>
	</div>
	<!-- 種別は後から変えられないので、フォームではなくここに出す。
	     区切りの「・」は見た目だけのものなので読み上げない。 -->
	<p class="flex flex-wrap items-center gap-x-1 text-sm text-muted-foreground">
		<TypeIcon class="size-4 shrink-0" aria-hidden="true" />
		<span>{contentTypeLabel(content.type)}</span>
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
		{#if content.extensions}
			<span aria-hidden="true">・</span>
			<span>{m.contents_form_extensions_label()}: {content.extensions}</span>
		{/if}
	</p>
	{#if viewOnlyNote}
		<p
			class="mt-3 flex max-w-xl items-start gap-2 rounded-md border bg-muted/40 px-3 py-2 text-sm"
			data-testid="view-only-note"
		>
			<EyeIcon class="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
			<span>{viewOnlyNote}</span>
		</p>
	{/if}
</div>

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
