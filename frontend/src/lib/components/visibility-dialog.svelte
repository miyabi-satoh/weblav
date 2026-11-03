<script lang="ts" module>
	import type { components } from '$lib/api/schema';

	type Visibility = components['schemas']['Visibility'];

	export type VisibilityTarget = {
		id: number;
		title: string;
		visibility: Visibility;
		options: Visibility[];
	};
</script>

<script lang="ts">
	/**
	 * 閲覧側の一覧の行から、編集ページを開かずに公開範囲だけを変えるダイアログ (→ docs/ui.md「UI 全般」)。
	 * `target` に対象を入れると開き、閉じると `null` に戻す。選べる値は呼び出し側が
	 * `selectableVisibilities` で決めて `options` に入れる (他人のものに「作成者のみ」を出さないため)。
	 */
	import { client } from '$lib/api/client';
	import { visibilityLabel } from '$lib/content-labels';
	import { dialogFormClass } from '$lib/dialog-form';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { FormDirtyState } from '$lib/form-dirty.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import * as RadioGroup from '$lib/components/ui/radio-group';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';

	let {
		target = $bindable(),
		onsaved
	}: {
		target: VisibilityTarget | null;
		/** 変えた後の画面の更新 (一覧を取り直す)。 */
		onsaved: () => void | Promise<void>;
	} = $props();

	let value = $state<Visibility>('public');
	let saving = $state(false);
	const form = new FormDirtyState(() => ({ value }));
	const errorDialog = new ErrorDialogState();

	// 開くたびに、今の公開範囲を選んだ状態から始める。
	// 選んだ値は読まない (読むと、選ぶたびに今の値へ戻してしまう)。
	$effect.pre(() => {
		if (target === null) return;
		const visibility = target.visibility;
		untrack(() => {
			value = visibility;
			form.markPristine();
		});
	});

	async function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (saving || target === null) return;
		if (value === target.visibility) {
			target = null;
			return;
		}
		saving = true;
		const id = target.id;
		const done = await errorDialog.attempt(
			() =>
				client.PUT('/api/v1/contents/{id}/visibility', {
					params: { path: { id } },
					body: { visibility: value }
				}),
			() => onsaved()
		);
		saving = false;
		if (done !== null) target = null;
	}
</script>

<Dialog.Root
	bind:open={
		() => target !== null,
		(open) => {
			if (!open) target = null;
		}
	}
>
	<Dialog.Content
		interactOutsideBehavior={form.dirty ? 'ignore' : 'close'}
		escapeKeydownBehavior={form.dirty ? 'ignore' : 'close'}
	>
		<Dialog.Header>
			<Dialog.Title>
				{m.contents_visibility_dialog_title({ title: target?.title ?? '' })}
			</Dialog.Title>
		</Dialog.Header>
		<form onsubmit={handleSubmit} class={dialogFormClass}>
			<Dialog.Body>
				<RadioGroup.Root bind:value aria-label={m.contents_form_visibility_label()}>
					{#each target?.options ?? [] as option (option)}
						<Field.Field orientation="horizontal" class="min-h-9">
							<RadioGroup.Item value={option} id="visibility-dialog-{option}" />
							<Field.FieldLabel for="visibility-dialog-{option}" class="font-normal">
								{visibilityLabel(option)}
							</Field.FieldLabel>
						</Field.Field>
					{/each}
				</RadioGroup.Root>
			</Dialog.Body>
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (target = null)}>
					{m.action_cancel()}
				</Button>
				<LoadingButton type="submit" loading={saving}>{m.action_save()}</LoadingButton>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
