<script lang="ts" generics="T extends string">
	import { goto } from '$app/navigation';
	import * as m from '$lib/paraglide/messages.js';
	import * as Select from '$lib/components/ui/select';
	import BrowseSelectTrigger from '$lib/components/browse-select-trigger.svelte';

	type Props = {
		/** 今の並び順。URL クエリから読んだ値を渡す。 */
		value: T;
		/** 選べる並び順 (画面ごとに違う。→ `$lib/browse-sort`, `$lib/archive-sort`)。 */
		options: { value: T; label: () => string }[];
		/** 並び順を変えた先の URL を組み立てる。クエリ以外は呼び出し側が決める。 */
		href: (sort: T) => string;
	};

	let { value, options, href }: Props = $props();

	let label = $derived(options.find((option) => option.value === value)?.label() ?? '');
</script>

<!-- ホーム・グループ・フォルダー・アーカイブの一覧の並び順 (→ docs/ui.md「ホーム・グループ・フォルダー・アーカイブの並び順」)。
     見た目は絞り込みのドロップダウンと揃えるが、常に何かが選ばれているので差し色の強調はしない。 -->
<Select.Root
	type="single"
	bind:value={
		() => value,
		(next) =>
			// eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」)
			goto(href(next as T))
	}
>
	<BrowseSelectTrigger label={m.browse_sort_label()} value={label} />
	<Select.Content>
		{#each options as option (option.value)}
			<Select.Item value={option.value}>{option.label()}</Select.Item>
		{/each}
	</Select.Content>
</Select.Root>
