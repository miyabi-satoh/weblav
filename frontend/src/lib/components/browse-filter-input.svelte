<script lang="ts">
	import XIcon from '@lucide/svelte/icons/x';
	import * as m from '$lib/paraglide/messages.js';
	import { listFilter } from '$lib/list-filter.svelte';

	// 一覧のページ内の絞り込みの入力欄 (→ docs/ui.md「一覧の絞り込み」)。
	// 見た目は並び順・軸のドロップダウンと揃え、上に項目名、下に入力の2段にする。
	// 絞り込み中は、軸の絞り込みと同じく差し色で強調する。
	const filter = listFilter();
	let input = $state<HTMLInputElement | null>(null);

	function clear() {
		filter?.set('');
		input?.focus();
	}
</script>

{#if filter}
	<!-- 消すボタンを label の外に置く。中に置くと、ボタンの文言まで入力欄の名前に入るため。 -->
	<div
		class={[
			'flex h-14 w-40 items-center gap-1 rounded-lg border border-input bg-background pr-1 pl-4 transition-colors focus-within:border-ring focus-within:ring-3 focus-within:ring-ring/50 dark:bg-input/30',
			filter.active && 'border-primary bg-primary/10 dark:bg-primary/10'
		]}
	>
		<label class="flex min-w-0 flex-1 flex-col gap-px">
			<span class="text-xs {filter.active ? 'text-primary' : 'text-muted-foreground'}">
				{m.browse_filter_label()}
			</span>
			<input
				bind:this={input}
				type="text"
				autocomplete="off"
				enterkeyhint="search"
				class="w-full min-w-0 bg-transparent text-base text-sub-foreground outline-none"
				bind:value={() => filter.value, (next) => filter.set(next)}
			/>
		</label>
		{#if filter.value !== ''}
			<button
				type="button"
				class="inline-flex size-11 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:text-foreground"
				onclick={clear}
			>
				<XIcon class="size-4" />
				<span class="sr-only">{m.browse_filter_clear_button()}</span>
			</button>
		{/if}
	</div>
{/if}
