<script lang="ts">
	import type { components } from '#lib/api/schema.js';
	import * as m from '#lib/paraglide/messages.js';
	import * as Field from '#lib/components/ui/field/index.ts';
	import { Input } from '#lib/components/ui/input/index.ts';
	import * as RadioGroup from '#lib/components/ui/radio-group/index.ts';
	import IntegerInput from '#lib/components/integer-input.svelte';

	type AutoMode = components['schemas']['AutoMode'];

	// 自動で回す処理の時刻 (しない・前回から N 時間たったら・毎日 HH:MM を過ぎたら) の丸ボタンの組。
	// 一覧の表示を使うフォルダーの自動の再スキャンで使う (→ docs/archive.md「スキャン」)。
	let {
		id,
		label,
		intervalAfter,
		mode = $bindable(),
		hours = $bindable(),
		time = $bindable()
	}: {
		/** 欄の id の頭。丸ボタンと数の欄の id をここから作る。 */
		id: string;
		label: string;
		/** 「前回から N 時間」に続く言葉 (何の前回か)。 */
		intervalAfter: string;
		mode: AutoMode;
		hours: number;
		time: string;
	} = $props();

	/** 時間の上限。サーバーが受ける数 (`u32`) の上限で、利用者に選ばせる値の目安ではない。 */
	const MAX_HOURS = 4_294_967_295;
	// Field.Field の横並びはラベルを伸ばすので、文の途中に欄を置く行では言葉の幅に戻す。
	const sentenceRowClass = 'min-h-9 flex-wrap [&>[data-slot=field-label]]:flex-none';
</script>

<Field.FieldSet>
	<Field.FieldLegend variant="label" id="{id}-legend">{label}</Field.FieldLegend>
	<RadioGroup.Root bind:value={mode} aria-labelledby="{id}-legend">
		<Field.Field orientation="horizontal" class="min-h-9">
			<RadioGroup.Item value="off" id="{id}-off" />
			<Field.FieldLabel for="{id}-off" class="font-normal">{m.schedule_off()}</Field.FieldLabel>
		</Field.Field>
		<!-- 数の欄を文の途中に置く。押せる範囲を丸だけでなく文の言葉にも広げる。 -->
		<Field.Field orientation="horizontal" class={sentenceRowClass}>
			<RadioGroup.Item value="interval" id="{id}-interval" />
			<Field.FieldLabel for="{id}-interval" class="font-normal"
				>{m.schedule_interval_before()}</Field.FieldLabel
			>
			<IntegerInput
				id="{id}-hours"
				class="w-44"
				min={1}
				max={MAX_HOURS}
				unit={m.schedule_hours_unit({ count: hours })}
				label={m.schedule_hours_unit({ count: hours })}
				bind:value={hours}
				required={mode === 'interval'}
			/>
			<Field.FieldLabel for="{id}-interval" class="font-normal">{intervalAfter}</Field.FieldLabel>
		</Field.Field>
		<Field.Field orientation="horizontal" class={sentenceRowClass}>
			<RadioGroup.Item value="daily" id="{id}-daily" />
			<Field.FieldLabel for="{id}-daily" class="font-normal"
				>{m.schedule_daily_before()}</Field.FieldLabel
			>
			<Input
				id="{id}-time"
				type="time"
				class="w-40"
				aria-label={m.schedule_time_label()}
				bind:value={time}
				required={mode === 'daily'}
			/>
			{#if m.schedule_daily_after() !== ''}
				<Field.FieldLabel for="{id}-daily" class="font-normal"
					>{m.schedule_daily_after()}</Field.FieldLabel
				>
			{/if}
		</Field.Field>
	</RadioGroup.Root>
</Field.FieldSet>
