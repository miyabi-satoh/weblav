<script lang="ts">
	import { activeFieldClass, fieldLabelClass } from '$lib/filter-field';
	import XIcon from '@lucide/svelte/icons/x';
	import type { ClassValue } from 'svelte/elements';

	// 一覧の上の段に置く、語を打つ欄 (名前の絞り込み・検索の語)。
	// 見た目は並び順・軸のドロップダウンと揃え、上に項目名、下に入力の2段にする (→ docs/ui.md「UI 全般」)。
	// `active` のときは、軸の絞り込みと同じく差し色で強調する。

	type Props = {
		label: string;
		/** 打った語を消すボタンの、読み上げ用の名前。 */
		clearLabel: string;
		value: string;
		input?: HTMLInputElement | null;
		id?: string;
		active?: boolean;
		class?: ClassValue;
	};

	let {
		label,
		clearLabel,
		value = $bindable(),
		input = $bindable(null),
		id,
		active = false,
		class: className
	}: Props = $props();

	function clear() {
		value = '';
		input?.focus();
	}
</script>

<!-- 消すボタンを label の外に置く。中に置くと、ボタンの文言まで入力欄の名前に入るため。 -->
<div
	class={[
		'flex h-14 items-center gap-1 rounded-lg border border-input bg-background pr-1 pl-4 transition-colors focus-within:border-ring focus-within:ring-3 focus-within:ring-ring/50 dark:bg-input/30',
		active && activeFieldClass,
		className
	]}
>
	<label class="flex min-w-0 flex-1 flex-col gap-px">
		<span class={fieldLabelClass(active)}>{label}</span>
		<input
			bind:this={input}
			{id}
			type="text"
			autocomplete="off"
			enterkeyhint="search"
			class="w-full min-w-0 bg-transparent text-base text-sub-foreground outline-none"
			bind:value
		/>
	</label>
	{#if value !== ''}
		<button
			type="button"
			class="inline-flex size-11 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:text-foreground"
			onclick={clear}
		>
			<XIcon class="size-4" />
			<span class="sr-only">{clearLabel}</span>
		</button>
	{/if}
</div>
