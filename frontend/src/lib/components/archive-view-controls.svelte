<script lang="ts">
	import BrowseControls from '$lib/components/browse-controls.svelte';
	import type { Snippet } from 'svelte';
	import { goto } from '$app/navigation';
	import { withQuery } from '$lib/href';
	import * as m from '$lib/paraglide/messages.js';
	import { ARCHIVE_SORT_OPTIONS, archiveSortHref, type ArchiveSort } from '$lib/archive-sort';
	import { listFilter } from '$lib/list-filter.svelte';
	import * as Select from '$lib/components/ui/select';
	import BrowseSelectTrigger from '$lib/components/browse-select-trigger.svelte';
	import BrowseSortSelect from '$lib/components/browse-sort.svelte';
	import type { components } from '$lib/api/schema';
	import { formatNumber } from '$lib/format';

	type ArchiveAxis = components['schemas']['ArchiveAxisResponse'];

	// アーカイブの一覧の上の段 (並べ替え・名前の絞り込み・軸の絞り込み・リストとタイルの切り替え) と件数の行。
	// 閲覧の画面と管理画面のアイテムのタブで、閲覧者に見える姿を同じに保つため1つにする。
	let {
		axes,
		filters,
		sort,
		path,
		total,
		shown,
		hasItems,
		folderMissing,
		countOnlyWhenFiltered = false,
		extra
	}: {
		axes: ArchiveAxis[];
		/** クエリの軸の絞り込み (軸名 → 選択肢の表示名)。 */
		filters: Record<string, string>;
		sort: ArchiveSort;
		/** この一覧のページのパス。絞り込みと並べ替えは、ここにクエリを付けて移る。 */
		path: string;
		/** 軸で絞り込んだ後の件数。 */
		total: number;
		/** 画面の中の絞り込みで残る件数。絞り込んでいなければ `undefined`。 */
		shown?: number;
		hasItems: boolean;
		folderMissing: boolean;
		/** 絞り込んでいないときは件数を出さない (管理画面は、見出しに同じ件数がある)。 */
		countOnlyWhenFiltered?: boolean;
		/** 軸の後ろに足す絞り込み (管理画面の公開の状態)。 */
		extra?: Snippet;
	} = $props();

	const list = listFilter();

	/**
	 * クエリで選ばれている選択肢。どの選択肢にも一致しない値はサーバーが無視するので
	 * (→ docs/archive.md「エンドポイント一覧」)、画面も絞り込み中として扱わない。
	 */
	function selectedOption(axis: ArchiveAxis) {
		const current = filters[axis.name];
		return axis.options.find((option) => option.value === current);
	}

	let hasFilters = $derived(axes.some((axis) => selectedOption(axis) !== undefined));

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
		// 軸を選び直しても、打った絞り込みは残す。
		const nextFilters: Record<string, string> = { ...filters, ...list?.query };
		if (value === allValueFor(axis)) {
			delete nextFilters[axis.name];
		} else {
			nextFilters[axis.name] = value;
		}
		return withQuery(path, nextFilters);
	}

	function handleAxisChange(axis: ArchiveAxis, value: string) {
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」)
		goto(filterHref(axis, value));
	}

	function sortHref(next: ArchiveSort): string {
		// 並び順を変えても、打った絞り込みは残す。
		return archiveSortHref(path, { ...filters, ...list?.query }, next);
	}

	// 件数は見出しにあるので、絞り込んでいないときは行ごと出さない。
	let hideCount = $derived(countOnlyWhenFiltered && !hasFilters && shown === undefined);

	// 軸の絞り込みだけを解除し、名前の絞り込みは残す。別の欄で打ったもので、解除の対象に見えないため。
	let resetHref = $derived(withQuery(path, list?.query ?? {}));
</script>

{#snippet sortSelect()}
	<BrowseSortSelect value={sort} options={ARCHIVE_SORT_OPTIONS} href={sortHref} />
{/snippet}

{#if hasItems || axes.length > 0}
	<!-- 絞り込みで0件になっても並び順と絞り込みは残すが、並べる物が無いので切り替えは出さない。 -->
	<BrowseControls layoutToggle={hasItems} sort={sortSelect}>
		{#each axes as axis (axis.name)}
			{@const filtered = selectedOption(axis) !== undefined}
			<Select.Root
				type="single"
				bind:value={() => axisValue(axis), (v) => handleAxisChange(axis, v)}
			>
				<BrowseSelectTrigger label={axis.name} value={axisLabel(axis)} highlighted={filtered} />
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
		{@render extra?.()}
	</BrowseControls>
{/if}

<!-- フォルダーが無いときは、件数を数える物が無い (下に「フォルダーが見つかりません。」だけを出す)。 -->
{#if !folderMissing && !hideCount}
	<div class="mt-2 flex min-h-11 items-center justify-between gap-3 text-sm">
		<p class="text-muted-foreground">
			{shown !== undefined
				? m.archive_view_count_filtered({
						shown: formatNumber(shown),
						count: formatNumber(total)
					})
				: m.archive_view_count({ count: formatNumber(total) })}
		</p>
		{#if hasFilters}
			<!-- パンくずと同じく実URL遷移にする。ブラウザーの戻る/進むを機能させるため。 -->
			<!-- eslint-disable svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」) -->
			<a
				href={resetHref}
				class="inline-flex h-11 items-center px-1 text-primary underline underline-offset-4"
			>
				{m.archive_view_reset_filters()}
			</a>
			<!-- eslint-enable svelte/no-navigation-without-resolve -->
		{/if}
	</div>
{/if}
