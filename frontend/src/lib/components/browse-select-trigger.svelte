<script lang="ts">
	import type { ComponentProps } from 'svelte';
	import * as Select from '$lib/components/ui/select';

	type Props = Omit<ComponentProps<typeof Select.Trigger>, 'children'> & {
		/** 上に小さく出す項目名。読み上げの名前にも使う。 */
		label: string;
		/** 下に出す、選んでいる値。 */
		value: string;
		/** 差し色で強調するか。絞り込み中の軸だけ (→ docs/ui.md「UI 全般」)。 */
		highlighted?: boolean;
	};

	let { label, value, highlighted = false, class: className, ...restProps }: Props = $props();
</script>

<!-- 閲覧画面の絞り込みと並び順のドロップダウン。上に項目名、下に値の2段 (→ docs/ui.md「UI 全般」)。
     高さは data-size の既定を上書きするため同じ variant で指定する。
     チェブロンは部品側で色が固定されているので子孫セレクタで塗り替える。 -->
<Select.Trigger
	{...restProps}
	aria-label={label}
	class={[
		'min-w-30 gap-3 px-4 data-[size=default]:h-14',
		highlighted && 'border-primary bg-primary/10 dark:bg-primary/10 [&_svg]:text-primary',
		className
	]}
>
	<span class="flex flex-col items-start gap-px">
		<span class="text-xs {highlighted ? 'text-primary' : 'text-muted-foreground'}">{label}</span>
		<span class="text-base {highlighted ? 'text-primary' : 'text-sub-foreground'}">{value}</span>
	</span>
</Select.Trigger>
