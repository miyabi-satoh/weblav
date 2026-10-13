<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { invalidateAll } from '$app/navigation';
	import { page } from '$app/state';
	import { client } from '$lib/api/client';
	import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
	import type { components } from '$lib/api/schema';
	import { refreshAiLinks } from '$lib/ai-links';
	import { formatTimeAgo } from '$lib/format';
	import { linkDetailOf, linkTitleOf, type LinkDetail } from '$lib/link-detail.svelte';
	import { RefreshedLinkPreviews } from '$lib/link-previews.svelte';
	import { listFilter } from '$lib/list-filter.svelte';
	import { browseGutterClass, browseItemClass, browseListClass } from '$lib/list-row';
	import { pageEmptyTextClass } from '$lib/page-layout';
	import * as m from '$lib/paraglide/messages.js';
	import ListRowLink from '$lib/components/list-row-link.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';

	type AiLinks = components['schemas']['AiLinksResponse'];
	type AiLink = AiLinks['links'][number];

	// グループの中身の下に並べる、AI が集めたリンク (→ docs/ai.md「AI が集めるリンク」)。
	// 行はリンクのカードと同じ形で、押すと詳しい表示を出す。そこからグループに登録できる。
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

	// ページ内の絞り込みが置かれていれば、グループの中身と同じ言葉で絞る (→ docs/ui.md「一覧の絞り込み」)。
	const filter = listFilter();
	let rows = $derived(filter ? filter.apply(aiLinks.links, titleOf) : aiLinks.links);

	// 登録できるのはログインしている人だけ (コンテンツの作成と同じ)。
	let canPin = $derived(page.data.user != null);
	let pinning = false;

	async function pin(link: AiLink) {
		if (pinning) return;
		pinning = true;
		try {
			const { response, error } = await client.POST('/api/v1/contents', {
				body: { type: 'link', parentId: groupId, url: link.url, title: titleOf(link) }
			});
			if (!response.ok) {
				toast.error(errorMessage(error));
				return;
			}
			toast.success(m.group_ai_links_pinned_toast());
			// グループの中身に足され、AI の分からは消える。
			await invalidateAll();
		} catch {
			toast.error(GENERIC_ERROR_MESSAGE());
		} finally {
			pinning = false;
		}
	}

	let details = $derived<LinkDetail[]>(
		rows.map((link) => ({
			...linkDetailOf({ href: link.url, title: titleOf(link), preview: previewOf(link) }),
			...(canPin
				? { action: { label: m.group_ai_links_pin_button(), run: () => void pin(link) } }
				: {})
		}))
	);

	let refreshing = $state(false);

	async function refresh() {
		if (refreshing) return;
		refreshing = true;
		try {
			const { response, error } = await refreshAiLinks(groupId);
			if (!response.ok) {
				toast.error(errorMessage(error));
				return;
			}
			toast.success(m.group_ai_links_refreshed_toast());
			// 一覧の上の絞り込みと切り替えを出すかも変わるので、画面ごと取り直す。
			await invalidateAll();
		} catch {
			toast.error(GENERIC_ERROR_MESSAGE());
		} finally {
			refreshing = false;
		}
	}

	let refreshedAgo = $derived(
		aiLinks.refreshedAt == null
			? ''
			: formatTimeAgo(new Date(aiLinks.refreshedAt * 1000).toISOString())
	);
</script>

<section class="mt-8" aria-labelledby="ai-links-heading">
	<div class={['mb-4 flex flex-wrap items-center justify-between gap-3', browseGutterClass]}>
		<div>
			<h2 id="ai-links-heading" class="text-base font-semibold">{m.group_ai_links_heading()}</h2>
			{#if refreshedAgo !== ''}
				<p class="text-sm text-muted-foreground">
					{m.group_ai_links_refreshed({ time: refreshedAgo })}
				</p>
			{/if}
		</div>
		{#if aiLinks.canRefresh}
			<LoadingButton variant="outline" size="sm" loading={refreshing} onclick={refresh}>
				{m.group_ai_links_refresh_button()}
			</LoadingButton>
		{/if}
	</div>

	{#if aiLinks.refreshedAt == null}
		<p class={pageEmptyTextClass}>{m.group_ai_links_never()}</p>
	{:else if aiLinks.links.length === 0}
		<p class={pageEmptyTextClass}>{m.group_ai_links_empty()}</p>
	{:else}
		<ul class={browseListClass()}>
			{#each rows as link, index (link.url)}
				<li class={browseItemClass()}>
					<ListRowLink
						href={link.url}
						title={titleOf(link)}
						preview={previewOf(link)}
						details={{ items: details, index }}
					/>
				</li>
			{/each}
		</ul>
	{/if}
</section>
