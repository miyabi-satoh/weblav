<script lang="ts">
	import { client } from '$lib/api/client';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { formatByteSize, formatDateTime, formatNumber } from '$lib/format';
	import { listClass, listItemClass } from '$lib/list-row';
	import {
		adminEmptyTextClass,
		adminListBandClass,
		adminListRowGutterClass,
		adminPageClass,
		adminPageHeaderClass,
		pageHeadingTextClass
	} from '$lib/page-layout';
	import { pageTitle } from '$lib/page-title';
	import * as m from '$lib/paraglide/messages.js';
	import { shareOpLabel, shareOpPlaces, shareOpResultLabel } from '$lib/share-operations';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import SeparatedText from '$lib/components/separated-text.svelte';
	import type { components } from '$lib/api/schema';
	import type { PageProps } from './$types';

	type ShareOperation = components['schemas']['ShareOperationResponse'];

	// 共有の中を書き換えた操作の記録 (→ docs/ui.md「UI 全般」)。誰がいつどこで何をしたかを新しい順に並べる。
	// スマートフォンの幅でも収まるよう、表にせず1件を数段に組む。
	let { data }: PageProps = $props();

	let more = $state<ShareOperation[]>([]);
	let nextBefore = $state<number | null>(null);
	let loadingMore = $state(false);
	const errorDialog = new ErrorDialogState();

	// 画面を開き直したら、足した分を捨てて最初の1回分からにする。
	$effect.pre(() => {
		nextBefore = data.operations.nextBefore ?? null;
		more = [];
	});

	let items = $derived([...data.operations.items, ...more]);

	async function loadMore() {
		if (loadingMore || nextBefore === null) return;
		loadingMore = true;
		const before = nextBefore;
		const result = await errorDialog.attempt(() =>
			client.GET('/api/v1/admin/share-operations', { params: { query: { before } } })
		);
		if (result?.data) {
			more = [...more, ...result.data.items];
			nextBefore = result.data.nextBefore ?? null;
		}
		loadingMore = false;
	}
</script>

<svelte:head><title>{pageTitle(m.admin_operations_nav_title())}</title></svelte:head>

<div class={adminPageClass}>
	<div class={adminPageHeaderClass}>
		<h1 class={pageHeadingTextClass}>{m.admin_operations_nav_title()}</h1>
	</div>

	{#if items.length === 0}
		<p class={adminEmptyTextClass}>{m.admin_operations_empty()}</p>
	{:else}
		<ul class={[adminListBandClass, listClass]}>
			{#each items as operation (operation.id)}
				{@const places = shareOpPlaces(operation)}
				{@const result = shareOpResultLabel(operation)}
				<li class={[listItemClass, adminListRowGutterClass, 'flex flex-col gap-0.5 py-3']}>
					<span class="flex flex-wrap items-baseline gap-x-2 text-sm">
						<span class="font-medium">{shareOpLabel(operation.op)}</span>
						<span class="text-muted-foreground">{operation.username}</span>
						{#if result}
							<span
								class={operation.result === 'failed' ? 'text-destructive' : 'text-muted-foreground'}
								>{result}</span
							>
						{/if}
					</span>
					<span class="text-xs wrap-anywhere"
						>{places.to === undefined ? places.from : `${places.from} → ${places.to}`}
					</span>
					<span class="text-xs text-muted-foreground">
						<SeparatedText
							text={[
								formatDateTime(operation.at),
								operation.itemCount == null
									? ''
									: m.admin_operations_items({ count: formatNumber(operation.itemCount) }),
								operation.byteCount == null ? '' : formatByteSize(operation.byteCount)
							].filter((part) => part !== '')}
						/>
					</span>
				</li>
			{/each}
		</ul>
		{#if nextBefore !== null}
			<div class="pt-4">
				<LoadingButton type="button" variant="outline" loading={loadingMore} onclick={loadMore}>
					{m.admin_operations_more()}
				</LoadingButton>
			</div>
		{/if}
	{/if}
</div>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
