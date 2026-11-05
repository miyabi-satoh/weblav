<script lang="ts">
	/**
	 * 共有の中で名前を1つ入れるダイアログ。新しいフォルダーと名前の変更で使う。
	 * 送るのは呼び出し側の `request`。失敗したら閉じずに理由を出し、入れ直せるようにする (同じ名前があるときなど)。
	 */
	import { tick, untrack } from 'svelte';
	import type { ApiResult } from '$lib/api/client';
	import { dialogFormClass } from '$lib/dialog-form';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { FormDirtyState } from '$lib/form-dirty.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';

	let {
		open = $bindable(),
		title,
		submitLabel,
		initialName = '',
		selectStem = false,
		request,
		ondone
	}: {
		open: boolean;
		title: string;
		submitLabel: string;
		/** 開いたときの名前。名前の変更では今の名前。 */
		initialName?: string;
		/** 開いたとき、拡張子の手前までを選んでおく (ファイルの名前の変更。OS のファイルの画面と同じ)。 */
		selectStem?: boolean;
		request: (name: string) => Promise<ApiResult>;
		/** 済んだ後の画面の更新。 */
		ondone: () => void | Promise<void>;
	} = $props();

	let name = $state('');
	let saving = $state(false);
	let inputElement = $state<HTMLInputElement | null>(null);
	const form = new FormDirtyState(() => ({ name }));
	const errorDialog = new ErrorDialogState();

	// 開くたびに初めの名前から始める。打っている名前は読まない (読むと、打つたびに初めの名前へ戻してしまう)。
	$effect.pre(() => {
		if (!open) return;
		const initial = initialName;
		untrack(() => {
			name = initial;
			form.markPristine();
		});
	});

	async function selectInitial() {
		await tick();
		if (!inputElement) return;
		inputElement.focus();
		const dot = selectStem ? name.lastIndexOf('.') : -1;
		// 「.」で始まる名前は拡張子だけに見えるので、全体を選ぶ。
		inputElement.setSelectionRange(0, dot > 0 ? dot : name.length);
	}

	async function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (saving) return;
		const trimmed = name.trim();
		if (trimmed === '' || trimmed === initialName) {
			if (trimmed === initialName) open = false;
			return;
		}
		saving = true;
		const done = await errorDialog.attempt(
			() => request(trimmed),
			() => ondone()
		);
		saving = false;
		if (done !== null) open = false;
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content
		interactOutsideBehavior={form.dirty ? 'ignore' : 'close'}
		escapeKeydownBehavior={form.dirty ? 'ignore' : 'close'}
		onOpenAutoFocus={(event) => {
			event.preventDefault();
			void selectInitial();
		}}
	>
		<Dialog.Header>
			<Dialog.Title>{title}</Dialog.Title>
		</Dialog.Header>
		<form onsubmit={handleSubmit} class={dialogFormClass}>
			<Dialog.Body>
				<Field.Field>
					<Field.FieldLabel for="share-entry-name" required
						>{m.shares_name_label()}</Field.FieldLabel
					>
					<Input
						id="share-entry-name"
						autocomplete="off"
						required
						bind:ref={inputElement}
						bind:value={name}
					/>
				</Field.Field>
			</Dialog.Body>
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (open = false)}>
					{m.action_cancel()}
				</Button>
				<LoadingButton type="submit" loading={saving} disabled={name.trim() === ''}>
					{submitLabel}
				</LoadingButton>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
