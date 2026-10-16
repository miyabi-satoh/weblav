<script lang="ts">
	import { Input } from '$lib/components/ui/input';
	import * as InputGroup from '$lib/components/ui/input-group';
	import { parseIntegerInput } from '$lib/integer-input';
	import * as m from '$lib/paraglide/messages.js';

	// `type="number"` を使わない (→ docs/ui.md「見た目の決めごと」)。
	// そのぶん範囲の検査を自前で持ち、ブラウザーの検証の吹き出しに載せる。
	type Props = {
		id: string;
		value: number;
		min: number;
		max: number;
		/** 枠の中の末尾に出す単位。 */
		unit?: string;
		required?: boolean;
		class?: string;
	};

	let {
		id,
		value = $bindable(),
		min,
		max,
		unit,
		required = false,
		class: className
	}: Props = $props();

	let ref = $state<HTMLInputElement | null>(null);
	// 打ちかけの文字列。値として読めない間も、打ったとおりに見せるため。
	// 打った時点の値を添え、外から値が変わったら (既定値に戻す・保存し直す) 捨てて値を見せる。
	let draft = $state<{ text: string; base: number } | null>(null);

	const text = $derived(draft && draft.base === value ? draft.text : String(value));
	const parsed = $derived(parseIntegerInput(text, min, max));

	$effect(() => {
		ref?.setCustomValidity(
			parsed.kind === 'invalid' ? m.integer_input_range_error({ min, max }) : ''
		);
	});

	function getText() {
		return text;
	}

	function setText(next: string) {
		const result = parseIntegerInput(next, min, max);
		if (result.kind === 'ok') value = result.value;
		draft = { text: next, base: value };
	}

	const unitId = $derived(`${id}-unit`);
</script>

{#if unit}
	<InputGroup.Root class={className}>
		<InputGroup.Input
			bind:ref
			{id}
			bind:value={getText, setText}
			inputmode="numeric"
			autocomplete="off"
			{required}
			aria-describedby={unitId}
		/>
		<InputGroup.Addon align="inline-end">
			<InputGroup.Text id={unitId}>{unit}</InputGroup.Text>
		</InputGroup.Addon>
	</InputGroup.Root>
{:else}
	<Input
		bind:ref
		{id}
		class={className}
		bind:value={getText, setText}
		inputmode="numeric"
		autocomplete="off"
		{required}
	/>
{/if}
