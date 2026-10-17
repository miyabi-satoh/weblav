<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { invalidateAll } from '$app/navigation';
	import { page } from '$app/state';
	import { client } from '$lib/api/client';
	import type { components } from '$lib/api/schema';
	import { aiLinkSite, autoRefreshAiLinks, refreshAiLinks } from '$lib/ai-links';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { urlHost } from '$lib/file-kind';
	import { formatTimeAgo } from '$lib/format';
	import { linkDetailOf, linkTitleOf, type LinkDetail } from '$lib/link-detail.svelte';
	import { RefreshedLinkPreviews } from '$lib/link-previews.svelte';
	import { listFilter } from '$lib/list-filter.svelte';
	import { browseGutterClass, browseItemClass, browseListClass } from '$lib/list-row';
	import { pageEmptyTextClass } from '$lib/page-layout';
	import * as m from '$lib/paraglide/messages.js';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { Button } from '$lib/components/ui/button';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import ListRowLink from '$lib/components/list-row-link.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import RowActionsMenu from '$lib/components/row-actions-menu.svelte';
	import SeparatedText from '$lib/components/separated-text.svelte';

	type AiLinks = components['schemas']['AiLinksResponse'];
	type AiLink = AiLinks['links'][number];
	type ExclusionKind = components['schemas']['ExclusionKind'];

	// グループの中身の下に並べる、AI が集めたリンク (→ docs/ai.md「AI が集めるリンク」)。
	// 行はリンクのカードと同じ形で、押すと詳しい表示を出す。行の「…」と詳しい表示から、残す・表示しないを選べる。
	let { groupId, aiLinks }: { groupId: number; aiLinks: AiLinks } = $props();

	// カードは覚えている情報ですぐ出し、サーバーに取り直しを頼んだ答えで差し替える (→ docs/ui.md「リンクのカード」)。
	const refreshed = new RefreshedLinkPreviews(() =>
		aiLinks.links.length > 0 ? { contentIds: [], aiLinksGroup: groupId } : undefined
	);

	function previewOf(link: AiLink) {
		return refreshed.get(link.url) ?? link.preview;
	}

	/** 検索で返ったタイトル。無ければ、ページのタイトル (無ければホスト名)。 */
	function titleOf(link: AiLink): string {
		return linkTitleOf(link.url, link.title || undefined, previewOf(link));
	}

	/** メニューと知らせに出すサイトの名前。行の2段目と同じく、サイト名 (無ければホスト名)。 */
	function siteNameOf(link: AiLink): string {
		return previewOf(link)?.siteName ?? urlHost(link.url);
	}

	/** この画面で表示しないにしたもの。押した行は「元に戻す」に替え、同じサイトのほかの行は外す。 */
	type Hidden = {
		id: number;
		kind: ExclusionKind;
		url: string;
		site: string | null;
		label: string;
	};
	let hidden = $state<Hidden[]>([]);

	function hiddenBy(link: AiLink): Hidden | undefined {
		const site = aiLinkSite(link.url);
		return hidden.find((h) =>
			h.kind === 'page' ? h.url === link.url : site !== null && h.site === site
		);
	}

	// ページ内の絞り込みが置かれていれば、グループの中身と同じ言葉で絞る (→ docs/ui.md「一覧の絞り込み」)。
	const filter = listFilter();
	let rows = $derived(filter ? filter.apply(aiLinks.links, titleOf) : aiLinks.links);
	// 表示しないにしたものは、詳しい表示の前後にも並べない。
	let shownRows = $derived(rows.filter((link) => hiddenBy(link) === undefined));

	// 残せるのはログインしている人だけ (コンテンツの作成と同じ)。表示しないはグループを書き換えられる人だけ。
	let canPin = $derived(page.data.user != null);
	let canHide = $derived(aiLinks.canEdit);

	let busy = false;
	const errorDialog = new ErrorDialogState();

	async function pin(link: AiLink) {
		if (busy) return;
		busy = true;
		try {
			await errorDialog.attempt(
				() =>
					client.POST('/api/v1/contents', {
						body: { type: 'link', parentId: groupId, url: link.url, title: titleOf(link) }
					}),
				async () => {
					toast.success(m.group_ai_links_pinned_toast());
					// グループの中身に足され、AI の分からは消える。
					await invalidateAll();
				}
			);
		} finally {
			busy = false;
		}
	}

	async function hide(link: AiLink, kind: ExclusionKind) {
		if (busy) return;
		busy = true;
		try {
			await errorDialog.attempt(
				() =>
					client.POST('/api/v1/contents/{id}/ai-links/exclusions', {
						params: { path: { id: groupId } },
						body: { url: link.url, kind }
					}),
				(result) => {
					if (!result.data) return;
					hidden.push({
						id: result.data.id,
						kind,
						url: link.url,
						site: aiLinkSite(link.url),
						label: kind === 'page' ? titleOf(link) : siteNameOf(link)
					});
				}
			);
		} finally {
			busy = false;
		}
	}

	async function undo(entry: Hidden) {
		if (busy) return;
		busy = true;
		try {
			await errorDialog.attempt(
				() =>
					client.DELETE('/api/v1/contents/{id}/ai-links/exclusions/{exclusion_id}', {
						params: { path: { id: groupId, exclusion_id: entry.id } }
					}),
				() => {
					hidden = hidden.filter((h) => h !== entry);
				}
			);
		} finally {
			busy = false;
		}
	}

	function menuOf(link: AiLink): { label: string; run: () => void }[] {
		if (!canHide) return [];
		return [
			{ label: m.group_ai_links_hide_page(), run: () => void hide(link, 'page') },
			{
				label: m.group_ai_links_hide_site({ site: siteNameOf(link) }),
				run: () => void hide(link, 'site')
			}
		];
	}

	function rowProps(link: AiLink) {
		return {
			href: link.url,
			title: titleOf(link),
			preview: previewOf(link),
			details: { items: details, index: shownRows.indexOf(link) },
			badge: link.isNew ? m.group_ai_links_new_badge() : undefined
		};
	}

	let details = $derived<LinkDetail[]>(
		shownRows.map((link) => ({
			...linkDetailOf({ href: link.url, title: titleOf(link), preview: previewOf(link) }),
			...(canPin
				? { action: { label: m.group_ai_links_pin_button(), run: () => void pin(link) } }
				: {}),
			menu: menuOf(link)
		}))
	);

	// 押した更新で新しいリンクが無かったか。「今日はここまで」と伝える。
	let noNew = $state(false);
	let refreshing = $state(false);

	async function refresh() {
		if (refreshing) return;
		refreshing = true;
		noNew = false;
		try {
			await errorDialog.attempt(
				() => refreshAiLinks(groupId),
				async (result) => {
					if (result.data?.added === 0) noNew = true;
					else toast.success(m.group_ai_links_refreshed_toast());
					// 一覧の上の絞り込みと切り替えを出すかも変わるので、画面ごと取り直す。
					await invalidateAll();
				}
			);
		} finally {
			refreshing = false;
		}
	}

	// 自動の更新 (→ docs/ai.md「自動の更新」)。前のリンクを見せたまま裏で取り直し、集め直したら差し替える。
	// 失敗はサーバーが飲み込んで今のリンクを返すので、画面には何も出さない。開くたびに1回だけ頼む。
	onMount(() => {
		if (!aiLinks.autoRefreshDue) return;
		void autoRefreshAiLinks(groupId)
			.then(async (result) => {
				if (result.data?.added != null) await invalidateAll();
			})
			.catch(() => {});
	});

	let refreshedText = $derived.by(() => {
		if (aiLinks.refreshedAt == null) return '';
		const time = formatTimeAgo(aiLinks.refreshedAt * 1000);
		return aiLinks.refreshedByAuto
			? m.group_ai_links_refreshed_auto({ time })
			: m.group_ai_links_refreshed({ time });
	});
	let statusParts = $derived(
		[
			refreshedText,
			aiLinks.refreshesLeft == null
				? ''
				: m.group_ai_links_refreshes_left({ count: aiLinks.refreshesLeft })
		].filter((part) => part !== '')
	);
	let limitReached = $derived(aiLinks.refreshesLeft === 0);
</script>

{#snippet rowMenu(link: AiLink)}
	<RowActionsMenu name={titleOf(link)}>
		{#if canPin}
			<DropdownMenu.Item onSelect={() => void pin(link)}>
				{m.group_ai_links_pin_button()}
			</DropdownMenu.Item>
		{/if}
		{#if canPin && canHide}
			<DropdownMenu.Separator />
		{/if}
		{#each menuOf(link) as item (item.label)}
			<DropdownMenu.Item onSelect={item.run}>{item.label}</DropdownMenu.Item>
		{/each}
	</RowActionsMenu>
{/snippet}

<section class="mt-8" aria-labelledby="ai-links-heading">
	<div class={['mb-4 flex flex-col gap-2', browseGutterClass]}>
		<div class="flex flex-wrap items-center justify-between gap-3">
			<div>
				<h2 id="ai-links-heading" class="text-base font-semibold">{m.group_ai_links_heading()}</h2>
				{#if statusParts.length > 0}
					<p class="text-sm text-muted-foreground"><SeparatedText text={statusParts} /></p>
				{/if}
			</div>
			{#if aiLinks.canRefresh}
				<LoadingButton
					variant="outline"
					size="sm"
					loading={refreshing}
					disabled={limitReached}
					onclick={refresh}
				>
					{m.group_ai_links_refresh_button()}
				</LoadingButton>
			{/if}
		</div>
		{#if aiLinks.canRefresh && limitReached}
			<p class="text-sm text-muted-foreground" role="status">{m.group_ai_links_limit_reached()}</p>
		{:else if noNew}
			<p class="text-sm text-muted-foreground" role="status">{m.group_ai_links_no_new()}</p>
		{/if}
	</div>
	{#if aiLinks.refreshedAt == null}
		<p class={pageEmptyTextClass}>{m.group_ai_links_never()}</p>
	{:else if aiLinks.links.length === 0}
		<p class={pageEmptyTextClass}>{m.group_ai_links_empty()}</p>
	{:else}
		<ul class={browseListClass()}>
			{#each rows as link (link.url)}
				{@const hiddenEntry = hiddenBy(link)}
				{#if hiddenEntry === undefined}
					<li class={browseItemClass()}>
						{#if canPin || canHide}
							<ListRowLink {...rowProps(link)}>
								{#snippet actions()}{@render rowMenu(link)}{/snippet}
							</ListRowLink>
						{:else}
							<ListRowLink {...rowProps(link)} />
						{/if}
					</li>
				{:else if hiddenEntry.url === link.url}
					<li class={browseItemClass()}>
						<div
							class={[
								'flex min-h-14 items-center justify-between gap-3 py-2 text-sm text-muted-foreground',
								browseGutterClass
							]}
							role="status"
						>
							<span class="min-w-0"
								>{hiddenEntry.kind === 'page'
									? m.group_ai_links_hidden_page({ title: hiddenEntry.label })
									: m.group_ai_links_hidden_site({ site: hiddenEntry.label })}</span
							>
							<Button variant="ghost" size="sm" onclick={() => void undo(hiddenEntry)}>
								{m.group_ai_links_undo_button()}
							</Button>
						</div>
					</li>
				{/if}
			{/each}
		</ul>
	{/if}
</section>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
