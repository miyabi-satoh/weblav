<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { client } from '$lib/api/client';
	import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { FormDirtyState } from '$lib/form-dirty.svelte';
	import { formatNumber } from '$lib/format';
	import { listClass, listItemClass } from '$lib/list-row';
	import { pageTitle } from '$lib/page-title';
	import { adminPageClass, pageHeadingTextClass } from '$lib/page-layout';
	import * as m from '$lib/paraglide/messages.js';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import ConfirmDialog from '$lib/components/confirm-dialog.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import RowActionsMenu from '$lib/components/row-actions-menu.svelte';
	import WarningBand from '$lib/components/warning-band.svelte';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import type { PageProps } from './$types';
	import type { components } from '$lib/api/schema';

	type Root = components['schemas']['RootResponse'];

	let { data }: PageProps = $props();

	// 窓を開いてから、選ばれたフォルダーを登録し終えるまで。
	let adding = $state(false);
	// 名前の変更ダイアログの対象。閉じるアニメーションの間も中身を出し続けるので、
	// 対象は閉じても消さず、開閉は別に持つ。
	let renaming = $state<Root | null>(null);
	let renamingOpen = $state(false);
	let nameInput = $state('');
	let savingName = $state(false);
	// 入力中のダイアログを外側クリックやEscで閉じてしまわないようにする
	// (コンテンツ管理・ユーザー管理のフォームと同じ扱い)。
	const nameForm = new FormDirtyState(() => ({ name: nameInput }));
	let deleting = $state(false);
	let deletingRoot = $state<Root | null>(null);
	const errorDialog = new ErrorDialogState();

	// このパソコンに OS のフォルダー選択の窓を出し、選んだらすぐ追加する (→ docs/folders.md「選び方」)。
	// 名前はサーバーがフォルダー名で付け、重なるときは連番を付ける (変えたいときは一覧の「名前の変更」)。
	async function handleAdd() {
		if (adding) return;
		adding = true;
		try {
			const path = await pick();
			// キャンセルか、失敗 (理由はダイアログに出してある)。
			if (path == null) return;
			await errorDialog.attempt(
				() => client.POST('/api/v1/admin/roots', { body: { path } }),
				() => invalidateAll()
			);
		} finally {
			adding = false;
		}
	}

	/** 窓で選ばれたパス。キャンセル・失敗なら `null`。 */
	async function pick(): Promise<string | null> {
		try {
			const { data, error, response } = await client.POST('/api/v1/admin/roots/pick');
			if (response.ok) return data?.path ?? null;
			// 窓がほかの画面から開かれたまま。ブラウザーの後ろに隠れていることもあるので、そう伝える。
			errorDialog.show(
				response.status === 409 ? m.admin_roots_picker_already_open() : errorMessage(error)
			);
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		}
		return null;
	}

	function openRename(root: Root) {
		nameInput = root.name;
		renaming = root;
		nameForm.markPristine();
		renamingOpen = true;
	}

	async function handleNameSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (renaming === null || savingName) return;
		const target = renaming;
		savingName = true;
		try {
			// 失敗したらダイアログは閉じない。名前が重なったときに、入れ直せるようにする。
			await errorDialog.attempt(
				() =>
					client.PUT('/api/v1/admin/roots/{id}', {
						params: { path: { id: target.id } },
						body: { name: nameInput }
					}),
				async () => {
					renamingOpen = false;
					await invalidateAll();
				}
			);
		} finally {
			savingName = false;
		}
	}

	async function handleDelete() {
		if (deletingRoot === null || deleting) return;
		deleting = true;
		const target = deletingRoot;
		try {
			await errorDialog.attempt(
				() => client.DELETE('/api/v1/admin/roots/{id}', { params: { path: { id: target.id } } }),
				async () => {
					deletingRoot = null;
					await invalidateAll();
				}
			);
		} finally {
			deleting = false;
		}
	}
</script>

<svelte:head><title>{pageTitle(m.admin_roots_title())}</title></svelte:head>

<div class={adminPageClass}>
	<!-- 追加ボタンは、ほかの管理画面 (コンテンツ・ユーザー) と同じく見出しの段の右に置く。 -->
	<div class="flex flex-wrap items-start justify-between gap-4">
		<div class="flex min-w-0 flex-col gap-2">
			<h1 class={pageHeadingTextClass}>{m.admin_roots_title()}</h1>
			<p class="text-sm text-muted-foreground">{m.admin_roots_description()}</p>
		</div>
		<LoadingButton loading={adding} onclick={handleAdd}>{m.admin_roots_add()}</LoadingButton>
	</div>
	<!-- 窓はブラウザーの外に出るので、どこで選ぶのかを添える。 -->
	<p class={['text-sm text-muted-foreground', adding && 'mt-2']} aria-live="polite">
		{adding ? m.admin_roots_picking() : ''}
	</p>
</div>

<WarningBand class="mb-4 md:px-6">
	<p class="text-sm leading-relaxed">{m.admin_roots_local_only_notice()}</p>
</WarningBand>

{#if data.roots.length === 0}
	<p class="px-4 text-sm text-muted-foreground md:px-6">{m.admin_roots_empty()}</p>
{:else}
	<ul class={listClass}>
		{#each data.roots as root (root.id)}
			<li class={[listItemClass, 'flex min-h-14 items-center gap-4 px-4 py-3 md:px-6']}>
				<!-- 名前と併せてフルパスも出す (→ docs/folders.md「公開できるフォルダー」)。 -->
				<div class="min-w-0 flex-1">
					<span class="block text-sm wrap-anywhere">{root.name}</span>
					<span class="mt-0.5 block text-xs wrap-anywhere text-muted-foreground">{root.path}</span>
					<span class="mt-0.5 block text-xs text-muted-foreground">
						{m.admin_roots_content_count({ count: formatNumber(root.contentCount) })}
					</span>
				</div>
				<RowActionsMenu name={root.name}>
					<DropdownMenu.Item onSelect={() => openRename(root)}>
						{m.admin_roots_rename_menu()}
					</DropdownMenu.Item>
					<DropdownMenu.Item variant="destructive" onSelect={() => (deletingRoot = root)}>
						{m.action_delete()}
					</DropdownMenu.Item>
				</RowActionsMenu>
			</li>
		{/each}
	</ul>
{/if}

<Dialog.Root bind:open={renamingOpen}>
	<Dialog.Content
		interactOutsideBehavior={nameForm.dirty ? 'ignore' : 'close'}
		escapeKeydownBehavior={nameForm.dirty ? 'ignore' : 'close'}
	>
		<Dialog.Header>
			<Dialog.Title>{m.admin_roots_rename_dialog_title()}</Dialog.Title>
		</Dialog.Header>
		<form onsubmit={handleNameSubmit} class="flex flex-col gap-6">
			<Field.FieldGroup>
				<Field.Field>
					<!-- 入力欄ではないので label にしない。 -->
					<Field.FieldTitle>{m.admin_roots_name_dialog_path_label()}</Field.FieldTitle>
					<p class="text-sm wrap-anywhere">{renaming?.path}</p>
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel for="root-name">{m.admin_roots_name_label()}</Field.FieldLabel>
					<Input id="root-name" autocomplete="off" bind:value={nameInput} />
				</Field.Field>
			</Field.FieldGroup>
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (renamingOpen = false)}
					>{m.action_cancel()}</Button
				>
				<LoadingButton type="submit" loading={savingName} disabled={!nameForm.dirty}
					>{m.action_save()}</LoadingButton
				>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<ConfirmDialog
	bind:open={
		() => deletingRoot !== null,
		(open) => {
			if (!open) deletingRoot = null;
		}
	}
	title={m.admin_roots_delete_confirm_title()}
	busy={deleting}
	onconfirm={handleDelete}
>
	{m.admin_roots_delete_confirm_description({ name: deletingRoot?.name ?? '' })}
	<!-- パスは長く、区切り以外に折り返せる場所が無いので、文から分けて小さく出し、どこででも折る
	     (`break-words` では最小幅が縮まず、狭い幅でダイアログを押し広げる)。 -->
	<span class="my-1 block text-xs wrap-anywhere">{deletingRoot?.path ?? ''}</span>
	{#if deletingRoot !== null && deletingRoot.removedContentCount > 0}
		{m.admin_roots_delete_confirm_contents_note({
			count: formatNumber(deletingRoot.removedContentCount)
		})}
	{/if}
</ConfirmDialog>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
