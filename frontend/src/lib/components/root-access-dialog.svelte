<script lang="ts" module>
	import type { components } from '$lib/api/schema';

	type ShareAccess = components['schemas']['ShareAccess'];

	/** ユーザーごとの選択。`default` は例外を置かない (既定に従う)。 */
	export type UserChoice = ShareAccess | 'default';

	/** 送る例外。既定と同じ値も例外として残す (後で既定を変えても、その人だけは変わらないように選んだため)。 */
	export function accessExceptions(
		choices: Record<number, UserChoice>
	): { userId: number; access: ShareAccess }[] {
		return Object.entries(choices)
			.filter((entry): entry is [string, ShareAccess] => entry[1] !== 'default')
			.map(([userId, access]) => ({ userId: Number(userId), access }));
	}
</script>

<script lang="ts">
	/**
	 * 共有ごとの権限 (→ docs/ui.md「UI 全般」の管理画面の共有)。一般ユーザーの既定と、ユーザーごとの例外を選ぶ。
	 * 管理者はどの共有でもすべての操作ができるので、並べるのは一般ユーザーだけ。
	 */
	import { untrack } from 'svelte';
	import { client } from '$lib/api/client';
	import { dialogFormClass } from '$lib/dialog-form';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { FormDirtyState } from '$lib/form-dirty.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import { ACCESS_CHOICES, shareAccessLabel } from '$lib/share-ops';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import * as Select from '$lib/components/ui/select';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';

	type Root = components['schemas']['RootResponse'];
	type UserListItem = components['schemas']['UserListItem'];

	let {
		root = $bindable(),
		users,
		onsaved
	}: {
		/** 権限を決める共有。入れると開き、閉じると `null` に戻す。 */
		root: Root | null;
		users: UserListItem[];
		onsaved: () => void | Promise<void>;
	} = $props();

	let editors = $derived(users.filter((user) => user.role === 'user'));
	let editorAccess = $state<ShareAccess>('read');
	let choices = $state<Record<number, UserChoice>>({});
	let saving = $state(false);
	const form = new FormDirtyState(() => ({ editorAccess, choices: { ...choices } }));
	const errorDialog = new ErrorDialogState();

	$effect.pre(() => {
		const current = root;
		if (current === null) return;
		untrack(() => {
			editorAccess = current.editorAccess;
			const next: Record<number, UserChoice> = {};
			for (const user of editors) next[user.id] = 'default';
			for (const exception of current.userAccess) next[exception.userId] = exception.access;
			choices = next;
			form.markPristine();
		});
	});

	function choiceLabel(choice: UserChoice): string {
		return choice === 'default' ? m.admin_roots_access_user_default() : shareAccessLabel(choice);
	}

	async function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (saving || root === null) return;
		const id = root.id;
		saving = true;
		const done = await errorDialog.attempt(
			() =>
				client.PUT('/api/v1/admin/roots/{id}/access', {
					params: { path: { id } },
					body: { editorAccess, users: accessExceptions(choices) }
				}),
			() => onsaved()
		);
		saving = false;
		if (done !== null) root = null;
	}
</script>

<Dialog.Root
	bind:open={
		() => root !== null,
		(open) => {
			if (!open) root = null;
		}
	}
>
	<Dialog.Content
		class="sm:max-w-lg"
		interactOutsideBehavior={form.dirty ? 'ignore' : 'close'}
		escapeKeydownBehavior={form.dirty ? 'ignore' : 'close'}
	>
		<Dialog.Header>
			<Dialog.Title>{m.admin_roots_access_dialog_title({ name: root?.name ?? '' })}</Dialog.Title>
		</Dialog.Header>
		<form onsubmit={handleSubmit} class={dialogFormClass}>
			<Dialog.Body class="flex flex-col gap-6">
				<Field.Field>
					<Field.FieldLabel for="root-access-default">
						{m.admin_roots_access_default_label()}
					</Field.FieldLabel>
					<!-- Field の中では欄が幅いっぱいに伸びるので、選択肢の長さに合わせた幅の箱で包む。 -->
					<div class="w-48">
						<Select.Root
							type="single"
							bind:value={() => editorAccess, (value) => (editorAccess = value as ShareAccess)}
						>
							<Select.Trigger id="root-access-default">
								{shareAccessLabel(editorAccess)}
							</Select.Trigger>
							<Select.Content>
								{#each ACCESS_CHOICES as access (access)}
									<Select.Item value={access}>{shareAccessLabel(access)}</Select.Item>
								{/each}
							</Select.Content>
						</Select.Root>
					</div>
					<Field.FieldDescription>{m.admin_roots_access_admin_note()}</Field.FieldDescription>
				</Field.Field>
				<div class="flex flex-col gap-2">
					<p class="text-sm font-medium">{m.admin_roots_access_users_label()}</p>
					{#if editors.length === 0}
						<p class="text-sm text-muted-foreground">{m.admin_roots_access_no_users()}</p>
					{:else}
						<ul class="flex flex-col">
							{#each editors as user (user.id)}
								{@const choice = choices[user.id] ?? 'default'}
								<li
									class="flex min-h-11 items-center gap-3 border-b border-divider py-1 last:border-b-0"
								>
									<span class="min-w-0 flex-1 text-sm wrap-anywhere">{user.username}</span>
									<Select.Root
										type="single"
										bind:value={() => choice, (value) => (choices[user.id] = value as UserChoice)}
									>
										<Select.Trigger
											class="w-40 shrink-0"
											aria-label={m.admin_roots_access_user_select({
												username: user.username,
												access: choiceLabel(choice)
											})}
										>
											{choiceLabel(choice)}
										</Select.Trigger>
										<Select.Content>
											<Select.Item value="default">{choiceLabel('default')}</Select.Item>
											{#each ACCESS_CHOICES as access (access)}
												<Select.Item value={access}>{shareAccessLabel(access)}</Select.Item>
											{/each}
										</Select.Content>
									</Select.Root>
								</li>
							{/each}
						</ul>
					{/if}
				</div>
			</Dialog.Body>
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (root = null)}>
					{m.action_cancel()}
				</Button>
				<LoadingButton type="submit" loading={saving} disabled={!form.dirty}>
					{m.action_save()}
				</LoadingButton>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
