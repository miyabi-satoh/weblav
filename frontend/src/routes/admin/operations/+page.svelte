<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
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
		adminTableBandClass,
		pageHeadingTextClass
	} from '$lib/page-layout';
	import { withQuery } from '$lib/href';
	import {
		OP_CATEGORIES,
		opCategoryLabel,
		opFieldLines,
		opLabel,
		opPlaces,
		opResultLabel
	} from '$lib/operations';
	import { pageTitle } from '$lib/page-title';
	import { fitColumnClass } from '$lib/table-columns';
	import { LatestRequest } from '$lib/latest-request';
	import * as m from '$lib/paraglide/messages.js';
	import * as Select from '$lib/components/ui/select';
	import * as Table from '$lib/components/ui/table';
	import BrowseSelectTrigger from '$lib/components/browse-select-trigger.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import SeparatedText from '$lib/components/separated-text.svelte';
	import type { components } from '$lib/api/schema';
	import type { PageProps } from './$types';

	type Operation = components['schemas']['OperationResponse'];

	// 操作の記録 (→ docs/ui.md「UI 全般」)。誰がいつ何に何をしたかを新しい順に並べる。
	let { data }: PageProps = $props();

	/** 絞り込みの「すべて」の値。Select の値は文字列なので、空を「絞り込まない」に充てる。 */
	const ALL = '';

	let categoryValue = $derived(data.filters.category ?? ALL);
	let userValue = $derived(data.filters.userId === undefined ? ALL : String(data.filters.userId));
	let userLabel = $derived(
		data.users.find((user) => String(user.id) === userValue)?.username ?? m.common_filter_all()
	);

	function filterHref(category: string, user: string): string {
		return withQuery(resolve('/admin/operations'), {
			...(category === ALL ? {} : { category }),
			...(user === ALL ? {} : { user })
		});
	}

	/** 共有の中の操作の件数と大きさ。ほかの操作には無い。 */
	function countParts(operation: Operation): string[] {
		return [
			operation.itemCount == null
				? ''
				: m.admin_operations_items({ count: formatNumber(operation.itemCount) }),
			operation.byteCount == null ? '' : formatByteSize(operation.byteCount)
		].filter((part) => part !== '');
	}

	let more = $state<Operation[]>([]);
	let nextBefore = $state<number | null>(null);
	let loadingMore = $state(false);
	const errorDialog = new ErrorDialogState();
	// 続きを読んでいる間に絞り込みを変えると、前の絞り込みの続きが後から届く。足さずに捨てる。
	const moreRequests = new LatestRequest();

	// 画面を開き直したら、足した分を捨てて最初の1回分からにする。
	$effect.pre(() => {
		nextBefore = data.operations.nextBefore ?? null;
		more = [];
		moreRequests.invalidate();
		loadingMore = false;
	});

	let items = $derived([...data.operations.items, ...more]);

	async function loadMore() {
		if (loadingMore || nextBefore === null) return;
		loadingMore = true;
		const before = nextBefore;
		const isLatest = moreRequests.begin();
		const result = await errorDialog.attempt(() =>
			client.GET('/api/v1/admin/operations', {
				params: { query: { ...data.filters, before } }
			})
		);
		if (!isLatest()) return;
		if (result?.data) {
			more = [...more, ...result.data.items];
			nextBefore = result.data.nextBefore ?? null;
		}
		loadingMore = false;
	}
</script>

{#snippet resultMark(operation: Operation)}
	{@const result = opResultLabel(operation)}
	{#if result}
		<span class={operation.result === 'failed' ? 'text-destructive' : 'text-muted-foreground'}
			>{result}</span
		>
	{/if}
{/snippet}

{#snippet targetLines(operation: Operation)}
	{@const places = opPlaces(operation)}
	{#if places}
		<span class="text-xs wrap-anywhere md:text-sm"
			>{places.to === undefined ? places.from : `${places.from} → ${places.to}`}</span
		>
	{/if}
	{#each opFieldLines(operation.fields ?? []) as line, index (index)}
		<span class="text-xs wrap-anywhere text-muted-foreground">{line}</span>
	{/each}
{/snippet}

<svelte:head><title>{pageTitle(m.admin_operations_nav_title())}</title></svelte:head>

<div class={adminPageClass}>
	<div class={adminPageHeaderClass}>
		<h1 class={pageHeadingTextClass}>{m.admin_operations_nav_title()}</h1>
	</div>

	<!-- 種類と、操作した人で絞り込む (GitHub・Google Workspace の監査ログと同じ2つ)。 -->
	<div class="flex flex-wrap gap-2 pb-4">
		<Select.Root
			type="single"
			bind:value={
				() => categoryValue,
				(next) =>
					// eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」)
					goto(filterHref(next, userValue))
			}
		>
			<BrowseSelectTrigger
				label={m.admin_operations_filter_category_label()}
				value={data.filters.category === undefined
					? m.common_filter_all()
					: opCategoryLabel(data.filters.category)}
				highlighted={categoryValue !== ALL}
			/>
			<Select.Content>
				<Select.Item value={ALL}>{m.common_filter_all()}</Select.Item>
				{#each OP_CATEGORIES as category (category)}
					<Select.Item value={category}>{opCategoryLabel(category)}</Select.Item>
				{/each}
			</Select.Content>
		</Select.Root>
		<Select.Root
			type="single"
			bind:value={
				() => userValue,
				(next) =>
					// eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」)
					goto(filterHref(categoryValue, next))
			}
		>
			<BrowseSelectTrigger
				label={m.admin_operations_filter_user_label()}
				value={userLabel}
				highlighted={userValue !== ALL}
			/>
			<Select.Content>
				<Select.Item value={ALL}>{m.common_filter_all()}</Select.Item>
				{#each data.users as user (user.id)}
					<Select.Item value={String(user.id)}>{user.username}</Select.Item>
				{/each}
			</Select.Content>
		</Select.Root>
	</div>

	{#if items.length === 0}
		<p class={adminEmptyTextClass}>
			{categoryValue === ALL && userValue === ALL
				? m.admin_operations_empty()
				: m.admin_operations_empty_filtered()}
		</p>
	{:else}
		<!-- スマートフォンの幅: 1件を数段に積む。列に並べると、どれかの列を隠すか横に送ることになるため。 -->
		<ul class={[adminListBandClass, listClass, 'md:hidden']}>
			{#each items as operation (operation.id)}
				<li class={[listItemClass, adminListRowGutterClass, 'flex flex-col gap-0.5 py-3']}>
					<span class="flex flex-wrap items-baseline gap-x-2 text-sm">
						<span class="font-medium">{opLabel(operation.op)}</span>
						<span class="text-muted-foreground">{operation.username}</span>
						{@render resultMark(operation)}
					</span>
					{@render targetLines(operation)}
					<span class="text-xs text-muted-foreground">
						<SeparatedText
							text={[
								formatDateTime(operation.at),
								...(operation.clientIp ? [operation.clientIp] : []),
								...countParts(operation)
							]}
						/>
					</span>
				</li>
			{/each}
		</ul>
		<!-- タブレットより広い幅: 監査ログの多くと同じく列に並べ、いつ・誰が・何を・何にを横に見比べられるようにする。
		     どの列も隠さない。 -->
		<div class={[adminTableBandClass, 'hidden md:block']}>
			<Table.Root>
				<Table.Header>
					<Table.Row>
						<Table.Head class={fitColumnClass}>{m.admin_operations_table_datetime()}</Table.Head>
						<Table.Head class={fitColumnClass}>{m.admin_operations_filter_user_label()}</Table.Head>
						<Table.Head class={fitColumnClass}>{m.admin_operations_table_operation()}</Table.Head>
						<Table.Head>{m.admin_operations_table_target()}</Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each items as operation (operation.id)}
						<Table.Row>
							<Table.Cell class={[fitColumnClass, 'align-top']}>
								<span class="block text-sm">{formatDateTime(operation.at)}</span>
								{#if operation.clientIp}
									<span class="block text-xs text-muted-foreground">{operation.clientIp}</span>
								{/if}
							</Table.Cell>
							<Table.Cell class={[fitColumnClass, 'align-top']}>{operation.username}</Table.Cell>
							<Table.Cell class={[fitColumnClass, 'align-top']}>
								<span class="block font-medium">{opLabel(operation.op)}</span>
								{@render resultMark(operation)}
							</Table.Cell>
							<Table.Cell class="align-top whitespace-normal">
								<div class="flex flex-col gap-0.5">
									{@render targetLines(operation)}
									{#if countParts(operation).length > 0}
										<span class="text-xs text-muted-foreground">
											<SeparatedText text={countParts(operation)} />
										</span>
									{/if}
								</div>
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</div>
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
