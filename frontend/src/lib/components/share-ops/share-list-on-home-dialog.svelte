<script lang="ts">
	/**
	 * 共有の中のフォルダーかファイルを、ショートカットとしてホームに載せる (→ docs/ui.md「共有の中の画面」)。
	 * 入れさせるのは公開範囲だけ。タイトルはサーバーが名前で付け、ほかは載せた後の編集ページで変える
	 * (→ docs/ui.md「UI 全般」の「作成の入力は必要最小限にする」)。
	 */
	import { client } from '#lib/api/client.ts';
	import type { components } from '#lib/api/schema.js';
	import { visibilityLabel } from '#lib/content-labels.ts';
	import { dialogFormClass } from '#lib/dialog-form.ts';
	import { ErrorDialogState } from '#lib/error-dialog-state.svelte.ts';
	import * as m from '#lib/paraglide/messages.js';
	import type { ShareTarget } from '#lib/share-ops.ts';
	import { Button } from '#lib/components/ui/button/index.ts';
	import * as Dialog from '#lib/components/ui/dialog/index.ts';
	import * as Field from '#lib/components/ui/field/index.ts';
	import * as RadioGroup from '#lib/components/ui/radio-group/index.ts';
	import ErrorDialog from '#lib/components/error-dialog.svelte';
	import LoadingButton from '#lib/components/loading-button.svelte';

	type Visibility = components['schemas']['Visibility'];

	/** ホームに出す選択肢だけを並べる。「非表示」で載せると、どこにも出ないものが増えるだけになるため。 */
	const OPTIONS: Visibility[] = ['public', 'authenticated', 'private'];

	let {
		target = $bindable(),
		shareId,
		ondone
	}: {
		target: ShareTarget | null;
		shareId: number;
		ondone: () => void | Promise<void>;
	} = $props();

	let value = $state<Visibility>('public');
	let saving = $state(false);
	const errorDialog = new ErrorDialogState();

	$effect.pre(() => {
		if (target !== null) value = 'public';
	});

	async function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (saving || target === null) return;
		const path = target.path;
		saving = true;
		const done = await errorDialog.attempt(
			() =>
				client.POST('/api/v1/shares/{id}/list-on-home', {
					params: { path: { id: shareId } },
					body: { path, visibility: value }
				}),
			() => ondone()
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
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>{m.shares_list_on_home_title({ name: target?.name ?? '' })}</Dialog.Title>
		</Dialog.Header>
		<form onsubmit={handleSubmit} class={dialogFormClass}>
			<Dialog.Body class="flex flex-col gap-2">
				<p id="list-on-home-visibility" class="font-medium">{m.contents_form_visibility_label()}</p>
				<RadioGroup.Root bind:value aria-labelledby="list-on-home-visibility">
					{#each OPTIONS as option (option)}
						<Field.Field orientation="horizontal" class="min-h-9">
							<RadioGroup.Item value={option} id="list-on-home-{option}" />
							<Field.FieldLabel for="list-on-home-{option}" class="font-normal">
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
				<LoadingButton type="submit" loading={saving}
					>{m.shares_list_on_home_submit()}</LoadingButton
				>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
