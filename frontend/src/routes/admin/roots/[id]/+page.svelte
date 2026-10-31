<script lang="ts">
	import { untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { refreshAll } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { client } from '$lib/api/client';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { FormDirtyState } from '$lib/form-dirty.svelte';
	import { formatNumber } from '$lib/format';
	import { gotoWithoutLeaveConfirm } from '$lib/leave-confirm';
	import { LeaveGuard } from '$lib/leave-guard.svelte';
	import { masonry } from '$lib/masonry';
	import {
		adminPageClass,
		pageHeadingInRowClass,
		sectionCardClass,
		sectionGridClass
	} from '$lib/page-layout';
	import { pageTitle } from '$lib/page-title';
	import * as m from '$lib/paraglide/messages.js';
	import { type UserChoice, accessChoices, accessExceptions } from '$lib/root-access';
	import { canAddRoots } from '$lib/root-location';
	import { pickRootFolder } from '$lib/root-picker';
	import { ACCESS_CHOICES, shareAccessLabel } from '$lib/share-ops';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import * as Select from '$lib/components/ui/select';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LeaveConfirmDialog from '$lib/components/leave-confirm-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import LoadingLabel from '$lib/components/loading-label.svelte';
	import RootDeleteDialog from '$lib/components/root-delete-dialog.svelte';
	import RowActionsMenu from '$lib/components/row-actions-menu.svelte';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
	import type { components } from '$lib/api/schema';
	import type { PageProps } from './$types';

	type Root = components['schemas']['RootResponse'];
	type ShareAccess = components['schemas']['ShareAccess'];
	/** 保存ボタンを持つ区画。場所は窓で選んだらすぐ変えるので、保存ボタンを持たない。 */
	type Part = 'name' | 'access';

	let { data }: PageProps = $props();

	let root = $derived(data.root);
	let editors = $derived(data.users.filter((user) => user.role === 'user'));
	// 場所を選び直すのは OS のフォルダー選択の窓なので、サーバーの PC から開いたときだけ出す (→ docs/folders.md「選び方」)。
	let canPick = $derived(canAddRoots(page.url.hostname));

	// 区画ごとに保存する (→ docs/ui.md「編集ページの区画」)。保存した後は応答の値で入れ直すので、読み込みは初回だけでよい。
	let name = $state(untrack(() => data.root.name));
	const nameForm = new FormDirtyState(() => ({ name }));
	nameForm.markPristine();

	let editorAccess = $state<ShareAccess>(untrack(() => data.root.editorAccess));
	let choices = $state<Record<number, UserChoice>>(
		untrack(() => accessChoices(data.root, data.users))
	);
	const accessForm = new FormDirtyState(() => ({ editorAccess, choices: { ...choices } }));
	accessForm.markPristine();

	let savingPart = $state<Part | null>(null);
	let saving = $derived(savingPart !== null);
	let picking = $state(false);
	let deletingRoot = $state<Root | null>(null);
	/** 確認を出している、選び直した先のパス。 */
	let movingTo = $state<string | null>(null);
	let moving = $state(false);
	const errorDialog = new ErrorDialogState();

	const leaveGuard = new LeaveGuard(
		() => nameForm.dirty || accessForm.dirty,
		() => saving
	);

	function choiceLabel(choice: UserChoice): string {
		return choice === 'default' ? m.admin_roots_access_user_default() : shareAccessLabel(choice);
	}

	/** 保存の後に、何を保存したかを知らせ、見出しを取り直す。保存中に止めた移動は、変更が残っていなければ続ける。 */
	async function afterSave(saved: boolean, section: string) {
		if (saved) {
			toast.success(m.common_section_saved_toast({ section }));
			await refreshAll();
		}
		await leaveGuard.resumeAfterSave(() => nameForm.dirty || accessForm.dirty || errorDialog.open);
	}

	async function handleNameSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (saving || name.trim() === '') return;
		savingPart = 'name';
		let saved = false;
		try {
			// 失敗したら値を残す。名前が重なったときに、入れ直せるようにする。
			const result = await errorDialog.attempt(() =>
				client.PUT('/api/v1/admin/roots/{id}', {
					params: { path: { id: root.id } },
					body: { name }
				})
			);
			if (result?.data) {
				// サーバーが前後の空白を落とすので、保存された値で入れ直す。
				name = result.data.name;
				nameForm.markPristine();
				saved = true;
			}
		} finally {
			savingPart = null;
		}
		await afterSave(saved, m.admin_roots_name_label());
	}

	async function handleAccessSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (saving) return;
		savingPart = 'access';
		let saved = false;
		try {
			const result = await errorDialog.attempt(() =>
				client.PUT('/api/v1/admin/roots/{id}/access', {
					params: { path: { id: root.id } },
					body: { editorAccess, users: accessExceptions(choices) }
				})
			);
			if (result?.data) {
				editorAccess = result.data.editorAccess;
				choices = accessChoices(result.data, data.users);
				accessForm.markPristine();
				saved = true;
			}
		} finally {
			savingPart = null;
		}
		await afterSave(saved, m.admin_roots_edit_access_title());
	}

	// 追加と同じ OS の窓で選び直す。違うフォルダーなら、中のコンテンツも移るので確かめてから変える。
	// 同じフォルダーなら、何も変えずに書き込みの許可だけを取り直す (macOS → docs/shares.md「共有の場所の選び直し」)。
	async function handlePick() {
		if (picking) return;
		picking = true;
		try {
			const path = await pickRootFolder(errorDialog);
			if (path == null) return;
			if (path === root.path) {
				const done = await move(path);
				if (done) toast.success(m.admin_roots_edit_path_unchanged_toast());
				return;
			}
			movingTo = path;
		} finally {
			picking = false;
		}
	}

	async function move(path: string): Promise<boolean> {
		const result = await errorDialog.attempt(
			() =>
				client.PUT('/api/v1/admin/roots/{id}/path', {
					params: { path: { id: root.id } },
					body: { path }
				}),
			() => refreshAll()
		);
		return result !== null;
	}

	async function confirmMove() {
		if (movingTo === null || moving) return;
		moving = true;
		try {
			await move(movingTo);
		} finally {
			moving = false;
			// 成否によらず閉じる。失敗の理由はエラーダイアログに出る。
			movingTo = null;
		}
	}
</script>

<svelte:head><title>{pageTitle(root.name)}</title></svelte:head>

<div class="px-4 pt-4 md:px-6">
	<a
		href={resolve('/admin/roots')}
		class="relative inline-flex items-center gap-1 text-sm text-muted-foreground after:absolute after:inset-x-0 after:-inset-y-3 hover:text-foreground"
	>
		<ChevronLeftIcon class="size-4" />
		<span class="underline underline-offset-4">{m.admin_roots_nav_title()}</span>
	</a>
	<!-- 削除はコンテンツの編集ページと同じく、見出しの段の⋮に入れる (→ docs/ui.md「UI 全般」)。 -->
	<div class="mt-2 flex items-start gap-2">
		<h1 class={pageHeadingInRowClass}>{root.name}</h1>
		<RowActionsMenu name={root.name}>
			<DropdownMenu.Item variant="destructive" onSelect={() => (deletingRoot = root)}>
				{m.action_delete()}
			</DropdownMenu.Item>
		</RowActionsMenu>
	</div>
	<p class="text-sm text-muted-foreground">
		{m.admin_roots_content_count({ count: formatNumber(root.contentCount) })}
	</p>
	<!-- 一覧の行と同じ知らせを、ここでも見出しの下に出す。 -->
	{#if root.status !== 'ready'}
		<p class="mt-1 flex items-start gap-1.5 text-sm text-warning-muted">
			<TriangleAlertIcon class="mt-0.5 size-4 shrink-0 text-warning-icon" />
			{root.status === 'unreachable'
				? m.admin_roots_status_unreachable()
				: m.admin_roots_status_missing()}
		</p>
	{/if}
	{#if !root.writable}
		<p class="mt-1 flex items-start gap-1.5 text-sm text-warning-muted">
			<TriangleAlertIcon class="mt-0.5 size-4 shrink-0 text-warning-icon" />
			{m.admin_roots_not_writable()}
		</p>
	{/if}
</div>

<div class={adminPageClass}>
	<!-- 場所・名前・権限を区画に分け、コンテンツの編集ページと同じ石垣に並べる (→ docs/ui.md「編集ページの区画」)。 -->
	<div class={sectionGridClass} {@attach masonry}>
		<section aria-labelledby="location-section-heading" class={sectionCardClass}>
			<h2 id="location-section-heading" class="text-base font-semibold">
				{m.admin_roots_edit_location_title()}
			</h2>
			<div class="flex flex-col gap-2">
				<!-- パスは区切り以外に折り返せる場所が無いので、どこでも折り返す。 -->
				<p class="text-sm wrap-anywhere" data-testid="root-path">{root.path}</p>
				<p class="text-sm text-muted-foreground">{m.admin_roots_edit_location_description()}</p>
			</div>
			{#if canPick}
				<div class="flex flex-col items-start gap-2">
					<LoadingButton variant="outline" loading={picking} disabled={moving} onclick={handlePick}
						>{m.admin_roots_edit_repick_button()}</LoadingButton
					>
					{#if !root.writable}
						<p class="text-sm text-muted-foreground">{m.admin_roots_edit_not_writable_note()}</p>
					{/if}
				</div>
			{:else}
				<p class="text-sm text-muted-foreground">{m.admin_roots_edit_location_local_only()}</p>
			{/if}
		</section>

		<form
			onsubmit={handleNameSubmit}
			aria-labelledby="name-section-heading"
			class={sectionCardClass}
		>
			<h2 id="name-section-heading" class="text-base font-semibold">
				{m.admin_roots_name_label()}
			</h2>
			<Field.Field>
				<Field.FieldLabel for="root-name" required>{m.admin_roots_name_label()}</Field.FieldLabel>
				<Input id="root-name" autocomplete="off" bind:value={name} required />
				<Field.FieldDescription>{m.admin_roots_edit_name_description()}</Field.FieldDescription>
			</Field.Field>
			<div>
				<LoadingButton
					type="submit"
					loading={savingPart === 'name'}
					disabled={saving || !nameForm.dirty || name.trim() === ''}
					>{m.action_save()}</LoadingButton
				>
			</div>
		</form>

		<!-- 管理者はどの共有でもすべての操作ができるので、例外に並べるのは一般ユーザーだけ (→ docs/shares.md「できること」)。 -->
		<form
			onsubmit={handleAccessSubmit}
			aria-labelledby="access-section-heading"
			class={sectionCardClass}
		>
			<h2 id="access-section-heading" class="text-base font-semibold">
				{m.admin_roots_edit_access_title()}
			</h2>
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
			<div>
				<LoadingButton
					type="submit"
					loading={savingPart === 'access'}
					disabled={saving || !accessForm.dirty}>{m.action_save()}</LoadingButton
				>
			</div>
		</form>
	</div>
</div>

<!-- 場所を変えると、中のコンテンツの登録先も移る。窓で選んだ後に、移る先と件数を見せてから変える。 -->
<AlertDialog.Root
	bind:open={
		() => movingTo !== null,
		(open) => {
			if (!open && !moving) movingTo = null;
		}
	}
>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{m.admin_roots_edit_move_confirm_title()}</AlertDialog.Title>
			<AlertDialog.Description>
				{m.admin_roots_edit_move_confirm_description({ name: root.name })}
				<span class="my-1 block text-xs wrap-anywhere">{movingTo ?? ''}</span>
				{#if root.contentCount > 0}
					{m.admin_roots_edit_move_confirm_contents_note({
						count: formatNumber(root.contentCount)
					})}
				{/if}
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel disabled={moving}>{m.action_cancel()}</AlertDialog.Cancel>
			<AlertDialog.Action class="relative" disabled={moving} onclick={confirmMove}>
				<LoadingLabel loading={moving}>{m.admin_roots_edit_move_confirm_button()}</LoadingLabel>
			</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<!-- 消した共有の編集ページには戻れないので、一覧へ移る。消したものの変更は保存しようがないので、離れるときの確認は出さない。 -->
<RootDeleteDialog
	bind:root={deletingRoot}
	ondeleted={() => gotoWithoutLeaveConfirm(resolve('/admin/roots'))}
/>

<LeaveConfirmDialog
	guard={leaveGuard}
	ondiscard={() => {
		nameForm.markPristine();
		accessForm.markPristine();
	}}
/>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
