<script lang="ts">
	import EyeIcon from '@lucide/svelte/icons/eye';
	import EyeOffIcon from '@lucide/svelte/icons/eye-off';
	import * as InputGroup from '$lib/components/ui/input-group';
	import * as m from '$lib/paraglide/messages.js';
	import type { ComponentProps } from 'svelte';

	type Props = Omit<ComponentProps<typeof InputGroup.Input>, 'type' | 'files'> & {
		class?: string;
	};

	let {
		ref = $bindable(null),
		value = $bindable(''),
		class: className,
		...restProps
	}: Props = $props();

	let visible = $state(false);
</script>

<InputGroup.Root class={className}>
	<!-- 実際の input の高さを外枠 (Root) に合わせ、入力欄の上下の余白をタップしてもフォーカスするようにする。
	     呼び出し側が外枠の高さを変えても追従する。 -->
	<InputGroup.Input
		bind:ref
		bind:value
		class="h-full"
		type={visible ? 'text' : 'password'}
		{...restProps}
	/>
	<InputGroup.Addon align="inline-end">
		<!-- 押せる範囲は 44px (→ docs/ui.md「UI 全般」)。入力欄の高さ (h-11) に合わせ、見た目は icon-sm。 -->
		<InputGroup.Button
			size="icon-sm"
			class="after:-inset-1.5"
			aria-label={visible ? m.password_input_hide_label() : m.password_input_show_label()}
			aria-pressed={visible}
			onclick={() => (visible = !visible)}
		>
			{#if visible}
				<EyeOffIcon />
			{:else}
				<EyeIcon />
			{/if}
		</InputGroup.Button>
	</InputGroup.Addon>
</InputGroup.Root>
