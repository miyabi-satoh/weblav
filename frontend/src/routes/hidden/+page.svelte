<script lang="ts">
	import { refreshAll } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { client } from '$lib/api/client';
	import { canEditContent } from '$lib/auth';
	import { contentTypeLabel, visibilityLabel } from '$lib/content-labels';
	import { byIdMap } from '$lib/content-tree';
	import { contentTypeIcon } from '$lib/content-types';
	import { contentViewHref } from '$lib/content-view-href';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import {
		browseGutterClass,
		browseItemClass,
		browseListClass,
		browseRowClass,
		browseRowPressClass,
		browseRowTextClass,
		browseRowTitleClass,
		rowMenuSlotClass
	} from '$lib/list-row';
	import { pageEmptyTextClass, pageHeadingClass } from '$lib/page-layout';
	import { pageTitle } from '$lib/page-title';
	import * as m from '$lib/paraglide/messages.js';
	import type { components } from '$lib/api/schema';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import ContentDeleteDialog from '$lib/components/content-delete-dialog.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import ListRowIcon from '$lib/components/list-row-icon.svelte';
	import ListRowSubtitle from '$lib/components/list-row-subtitle.svelte';
	import RowActionsMenu from '$lib/components/row-actions-menu.svelte';
	import type { PageProps } from './$types';

	type HiddenContent = components['schemas']['HiddenContentResponse'];
	type Content = components['schemas']['AdminContentResponse'];

	let { data }: PageProps = $props();

	let contentById = $derived(byIdMap(data.contents));
	let deleteTarget = $state<Content | null>(null);
	let busy = $state(false);
	const errorDialog = new ErrorDialogState();

	// 管理者には他人のものも並ぶので、誰のものかを添える。
	function subtitle(content: HiddenContent): string[] {
		const parts = [contentTypeLabel(content.type)];
		if (content.createdBy !== page.data.user?.id && content.createdByUsername) {
			parts.push(content.createdByUsername);
		}
		if (content.restoreVisibility) {
			parts.push(m.hidden_restore_to({ visibility: visibilityLabel(content.restoreVisibility) }));
		}
		return parts;
	}

	// 一覧の件数と、ユーザーメニューの件数 (`/auth/me`) をそろえて取り直す。
	async function unhide(content: HiddenContent) {
		if (busy) return;
		busy = true;
		try {
			await errorDialog.attempt(
				() => client.POST('/api/v1/contents/{id}/unhide', { params: { path: { id: content.id } } }),
				() => refreshAll()
			);
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>{pageTitle(m.hidden_title())}</title></svelte:head>

<div class="py-6">
	<div class={browseGutterClass}>
		<h1 class={pageHeadingClass}>{m.hidden_title()}</h1>
	</div>

	{#if data.hidden.length === 0}
		<p class={pageEmptyTextClass}>{m.hidden_empty()}</p>
	{:else}
		<ul class={browseListClass()}>
			{#each data.hidden as content (content.id)}
				{@const view = contentViewHref(content, page.data.user?.id)}
				<li class={browseItemClass()}>
					<div class={browseRowClass()}>
						<ListRowIcon icon={contentTypeIcon(content.type)} />
						<span class={browseRowTextClass()}>
							<!-- 非表示のものは閲覧の場に並ばないので、閲覧画面での見え方を確かめる道をここに置く (→ docs/ui.md「UI 全般」)。 -->
							{#if view.external}
								<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- 外のサイトの URL -->
								<a
									href={view.href}
									class={browseRowPressClass()}
									target="_blank"
									rel="external noopener noreferrer"
								>
									<span class={browseRowTitleClass()}>{content.title}</span>
									<span class="sr-only">{m.contents_opens_in_new_tab()}</span>
								</a>
							{:else}
								<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -- contentViewHref() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
								<a href={view.href} class={browseRowPressClass()}>
									<span class={browseRowTitleClass()}>{content.title}</span>
								</a>
							{/if}
							<ListRowSubtitle text={subtitle(content)} />
						</span>
						<span class={rowMenuSlotClass()}>
							<RowActionsMenu name={content.title}>
								{#if content.restoreVisibility}
									<DropdownMenu.Item disabled={busy} onSelect={() => void unhide(content)}>
										{m.hidden_unhide_menu()}
									</DropdownMenu.Item>
								{/if}
								<DropdownMenu.Item>
									{#snippet child({ props })}
										<a href={resolve('/admin/contents/[id]', { id: String(content.id) })} {...props}
											>{m.action_edit_page()}</a
										>
									{/snippet}
								</DropdownMenu.Item>
								{#if canEditContent(page.data.user, content)}
									<DropdownMenu.Item
										variant="destructive"
										onSelect={() => (deleteTarget = contentById.get(content.id) ?? content)}
									>
										{m.action_delete()}
									</DropdownMenu.Item>
								{/if}
							</RowActionsMenu>
						</span>
					</div>
				</li>
			{/each}
		</ul>
	{/if}
</div>

<ContentDeleteDialog
	bind:content={deleteTarget}
	contents={data.contents}
	{contentById}
	ondeleted={() => refreshAll()}
/>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
