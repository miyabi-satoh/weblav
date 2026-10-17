<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { invalidateAll } from '$app/navigation';
	import { client } from '$lib/api/client';
	import type { components } from '$lib/api/schema';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { listItemClass } from '$lib/list-row';
	import { sectionCardClass } from '$lib/page-layout';
	import * as m from '$lib/paraglide/messages.js';
	import { Button } from '$lib/components/ui/button';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import ErrorDialog from '$lib/components/error-dialog.svelte';

	type Exclusion = components['schemas']['AiLinkExclusion'];
	type ExclusionKind = components['schemas']['ExclusionKind'];

	// グループの編集ページの「表示しないもの」の区画 (→ docs/ai.md「AI が集めるリンク」)。
	// 「表示に戻す」はその場で効くので、保存ボタンを置かない。
	let {
		contentId,
		exclusions,
		editable
	}: { contentId: number; exclusions: Exclusion[]; editable: boolean } = $props();

	let kind = $state<ExclusionKind>('page');
	let shown = $derived(exclusions.filter((exclusion) => exclusion.kind === kind));
	let pageCount = $derived(exclusions.filter((exclusion) => exclusion.kind === 'page').length);
	let siteCount = $derived(exclusions.length - pageCount);

	/** 行の名前。ページはタイトル (無ければ URL)、サイトはホスト名。 */
	function nameOf(exclusion: Exclusion): string {
		return exclusion.kind === 'page' ? exclusion.title || exclusion.value : exclusion.value;
	}

	let restoring = $state<number | null>(null);
	const errorDialog = new ErrorDialogState();

	async function restore(exclusion: Exclusion) {
		if (restoring !== null) return;
		restoring = exclusion.id;
		try {
			await errorDialog.attempt(
				() =>
					client.DELETE('/api/v1/contents/{id}/ai-links/exclusions/{exclusion_id}', {
						params: { path: { id: contentId, exclusion_id: exclusion.id } }
					}),
				async () => {
					toast.success(m.contents_ai_exclusions_restored_toast());
					await invalidateAll();
				}
			);
		} finally {
			restoring = null;
		}
	}
</script>

<section aria-labelledby="ai-exclusions-heading" class={sectionCardClass}>
	<h2 id="ai-exclusions-heading" class="text-base font-semibold">
		{m.contents_ai_exclusions_title()}
	</h2>
	<div class="flex flex-col gap-3">
		<ToggleGroup.Root
			type="single"
			variant="outline"
			bind:value={() => kind, (value) => (kind = value === 'site' ? 'site' : 'page')}
			class="self-start"
		>
			<ToggleGroup.Item value="page"
				>{m.contents_ai_exclusions_pages_tab({ count: pageCount })}</ToggleGroup.Item
			>
			<ToggleGroup.Item value="site"
				>{m.contents_ai_exclusions_sites_tab({ count: siteCount })}</ToggleGroup.Item
			>
		</ToggleGroup.Root>
		{#if shown.length === 0}
			<p class="text-sm text-muted-foreground">
				{kind === 'page'
					? m.contents_ai_exclusions_pages_empty()
					: m.contents_ai_exclusions_sites_empty()}
			</p>
		{:else}
			<ul class="flex flex-col">
				{#each shown as exclusion (exclusion.id)}
					<li class={['flex min-h-11 items-center justify-between gap-3', listItemClass]}>
						<span class="min-w-0 text-sm wrap-anywhere">{nameOf(exclusion)}</span>
						{#if editable}
							<Button
								variant="ghost"
								size="sm"
								disabled={restoring !== null}
								aria-label={m.contents_ai_exclusions_restore_label({ name: nameOf(exclusion) })}
								onclick={() => void restore(exclusion)}
							>
								{m.contents_ai_exclusions_restore_button()}
							</Button>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</section>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
