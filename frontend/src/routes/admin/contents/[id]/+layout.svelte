<script lang="ts">
	import { pageHeadingTextClass } from '$lib/page-layout';
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import { navTabClass, navTabListClass } from '$lib/nav-tabs';
	import { pageTitle } from '$lib/page-title';
	import { contentTypeLabel } from '$lib/content-labels';
	import { contentTypeIcon } from '$lib/content-types';
	import { canManageRoots, contentPathLabel } from '$lib/root-location';
	import * as m from '$lib/paraglide/messages.js';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import EyeIcon from '@lucide/svelte/icons/eye';
	import type { LayoutProps } from './$types';

	let { data, children }: LayoutProps = $props();

	let content = $derived(data.content);

	// アーカイブは「基本情報 / 軸定義 / アイテム」を同列に並べる (→ docs/access.md「ロールと操作」)。
	let tabs = $derived.by(() => {
		const id = String(data.contentId);
		const basic = {
			href: resolve('/admin/contents/[id]', { id }),
			label: m.contents_edit_tabs_basic()
		};
		if (content.type !== 'archive') return [basic];
		return [
			basic,
			{ href: resolve('/admin/contents/[id]/axes', { id }), label: m.archive_tabs_axes() },
			{ href: resolve('/admin/contents/[id]/items', { id }), label: m.archive_tabs_items() }
		];
	});

	let location = $derived(
		content.url ??
			contentPathLabel(content, canManageRoots(page.data.user, page.url.hostname)) ??
			content.fileName
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
	<h1 class={['mt-2', pageHeadingTextClass]}>{content.title}</h1>
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
