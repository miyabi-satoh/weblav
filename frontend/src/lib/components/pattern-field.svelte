<script lang="ts">
	import { Button } from '#lib/components/ui/button/index.ts';
	import * as Dialog from '#lib/components/ui/dialog/index.ts';
	import * as Field from '#lib/components/ui/field/index.ts';
	import { Textarea } from '#lib/components/ui/textarea/index.ts';
	import { dialogFormClass } from '#lib/dialog-form.ts';
	import * as m from '#lib/paraglide/messages.js';

	type Props = {
		label: string;
		help: string;
		/** 何も書いていないときに出す語。 */
		emptyText: string;
		/** 1行に1パターン。 */
		value: string;
		/** 見せるだけで変えさせない (サーバーの PC の外から開いた管理画面など)。 */
		readonly?: boolean;
		/** 区画の欄がこれだけのとき、名前を区画の見出し (h2) にして同じ語を2度出さない。その見出しの id。 */
		headingId?: string;
	};

	let {
		label,
		help,
		emptyText,
		value = $bindable(),
		readonly = false,
		headingId
	}: Props = $props();

	let open = $state(false);
	let draft = $state('');

	// 普段は場所を取らないように1行に連結して見せ、行数の要る編集は窓で行う。
	let summary = $derived(
		value
			.split('\n')
			.map((line) => line.trim())
			.filter((line) => line !== '')
			.join(', ')
	);

	function startEditing() {
		draft = value;
		open = true;
	}

	function apply(event: SubmitEvent) {
		event.preventDefault();
		value = draft;
		open = false;
	}
</script>

<Field.Field>
	<!-- 入力欄ではないので label にしない。 -->
	{#if headingId}
		<h2 id={headingId} class="text-base font-semibold">{label}</h2>
	{:else}
		<Field.FieldTitle>{label}</Field.FieldTitle>
	{/if}
	<div class="flex items-center gap-3">
		<span class="min-w-0 flex-1 text-sm wrap-anywhere">
			{#if summary === ''}
				<span class="text-muted-foreground">{emptyText}</span>
			{:else}
				{summary}
			{/if}
		</span>
		{#if !readonly}
			<Button
				type="button"
				variant="outline"
				size="sm"
				aria-label={`${m.action_edit()} ${label}`}
				onclick={startEditing}
			>
				{m.action_edit()}
			</Button>
		{/if}
	</div>
</Field.Field>

<Dialog.Root bind:open>
	<Dialog.Content class="sm:max-w-lg">
		<form class={dialogFormClass} onsubmit={apply}>
			<Dialog.Header>
				<Dialog.Title>{label}</Dialog.Title>
				<Dialog.Description>{help}</Dialog.Description>
			</Dialog.Header>
			<Dialog.Body>
				<Textarea aria-label={label} class="h-60 font-mono" bind:value={draft} />
			</Dialog.Body>
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (open = false)}>
					{m.action_cancel()}
				</Button>
				<Button type="submit">{m.contents_form_patterns_apply()}</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
