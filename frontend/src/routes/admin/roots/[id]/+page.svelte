<script lang="ts">
	import { untrack } from 'svelte';
	import { refreshAll } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { client } from '#lib/api/client.ts';
	import { ErrorDialogState } from '#lib/error-dialog-state.svelte.ts';
	import { FormDirtyState } from '#lib/form-dirty.svelte.ts';
	import { formatNumber } from '#lib/format.ts';
	import { gotoWithoutLeaveConfirm } from '#lib/leave-confirm.ts';
	import { SectionSaveState } from '#lib/section-save.svelte.ts';
	import { masonry } from '#lib/masonry.ts';
	import {
		adminPageClass,
		pageHeadingTextClass,
		sectionCardClass,
		sectionGridClass
	} from '#lib/page-layout.ts';
	import { pageTitle } from '#lib/page-title.ts';
	import * as m from '#lib/paraglide/messages.js';
	import {
		type AccessException,
		addException,
		exceptionCandidates,
		visibleExceptions
	} from '#lib/root-access.ts';
	import { canAddRoots } from '#lib/root-location.ts';
	import { pickRootFolder } from '#lib/root-picker.ts';
	import { ACCESS_CHOICES, shareAccessLabel } from '#lib/share-ops.ts';
	import * as AlertDialog from '#lib/components/ui/alert-dialog/index.ts';
	import { Button, buttonVariants } from '#lib/components/ui/button/index.ts';
	import * as DropdownMenu from '#lib/components/ui/dropdown-menu/index.ts';
	import * as Field from '#lib/components/ui/field/index.ts';
	import { Input } from '#lib/components/ui/input/index.ts';
	import * as Select from '#lib/components/ui/select/index.ts';
	import ErrorDialog from '#lib/components/error-dialog.svelte';
	import LeaveConfirmDialog from '#lib/components/leave-confirm-dialog.svelte';
	import LoadingButton from '#lib/components/loading-button.svelte';
	import LoadingLabel from '#lib/components/loading-label.svelte';
	import RootDeleteDialog from '#lib/components/root-delete-dialog.svelte';
	import DeleteSection from '#lib/components/delete-section.svelte';
	import XIcon from '@lucide/svelte/icons/x';
	import BackLink from '#lib/components/back-link.svelte';
	import RootWarnings from '#lib/components/root-warnings.svelte';
	import { listItemClass } from '#lib/list-row.ts';
	import type { components } from '#lib/api/schema.js';
	import type { PageProps } from './$types';

	type Root = components['schemas']['RootResponse'];
	type ShareAccess = components['schemas']['ShareAccess'];
	/** 保存ボタンを持つ区画。場所は窓で選んだらすぐ変えるので、保存ボタンを持たない。 */
	type Part = 'name' | 'access';

	let { data }: PageProps = $props();

	let root = $derived(data.root);
	// 場所を選び直すのは OS のフォルダー選択の窓なので、サーバーの PC から開いたときだけ出す (→ docs/folders.md「選び方」)。
	let canPick = $derived(canAddRoots(page.url.hostname));

	// 区画ごとに保存する (→ docs/ui.md「編集ページの区画」)。保存した後は応答の値で入れ直すので、読み込みは初回だけでよい。
	let name = $state(untrack(() => data.root.name));
	const nameForm = new FormDirtyState(() => ({ name }));
	nameForm.markPristine();

	let editorAccess = $state<ShareAccess>(untrack(() => data.root.editorAccess));
	let exceptions = $state<AccessException[]>(untrack(() => data.root.userAccess));
	let exceptionRows = $derived(visibleExceptions(exceptions, data.users));
	let candidates = $derived(exceptionCandidates(data.users, exceptions));
	const accessForm = new FormDirtyState(() => ({ editorAccess, exceptions }));
	accessForm.markPristine();

	let picking = $state(false);
	let deletingRoot = $state<Root | null>(null);
	/** 確認を出している、選び直した先のパス。 */
	let movingTo = $state<string | null>(null);
	let moving = $state(false);
	const errorDialog = new ErrorDialogState();

	const sections = new SectionSaveState<Part>([nameForm, accessForm], errorDialog);

	function setExceptionAccess(target: AccessException, access: string) {
		exceptions = exceptions.map((exception) =>
			exception.userId === target.userId
				? { ...exception, access: access as ShareAccess }
				: exception
		);
	}

	function removeException(target: AccessException) {
		exceptions = exceptions.filter((exception) => exception.userId !== target.userId);
	}

	async function handleNameSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (name.trim() === '') return;
		await sections.save('name', m.admin_roots_name_label(), async () => {
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
				return true;
			}
			return false;
		});
	}

	async function handleAccessSubmit(event: SubmitEvent) {
		event.preventDefault();
		await sections.save('access', m.admin_roots_edit_access_title(), async () => {
			const result = await errorDialog.attempt(() =>
				client.PUT('/api/v1/admin/roots/{id}/access', {
					params: { path: { id: root.id } },
					body: {
						editorAccess,
						users: exceptions.map(({ userId, access }) => ({ userId, access }))
					}
				})
			);
			if (result?.data) {
				editorAccess = result.data.editorAccess;
				exceptions = result.data.userAccess;
				accessForm.markPristine();
				return true;
			}
			return false;
		});
	}

	// 追加と同じ OS の窓で選び直す。違うフォルダーなら、中のコンテンツも移るので確かめてから変える。
	// 同じフォルダーなら場所は変えない。起動したときに許可を戻せていなければ、ここで戻す (macOS → docs/shares.md「共有の場所の選び直し」)。
	async function handlePick() {
		if (picking) return;
		picking = true;
		try {
			const path = await pickRootFolder(errorDialog);
			if (path == null) return;
			// 場所は画面のまま変わらないので、知らせは出さない。
			if (path === root.path) {
				await move(path);
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
	<BackLink href={resolve('admin/roots')} label={m.admin_roots_nav_title()} />

	<!-- 削除は、ほかの編集ページと同じく最後の「削除」の区画に置く (→ docs/ui.md「編集ページの区画」)。 -->
	<h1 class={['mt-2 wrap-anywhere', pageHeadingTextClass]}>{root.name}</h1>
	<p class="text-sm text-muted-foreground">
		{m.admin_roots_content_count({ count: formatNumber(root.contentCount) })}
	</p>
	<!-- 一覧の行と同じ知らせを、ここでも見出しの下に出す。 -->
	<RootWarnings status={root.status} size="sm" />
</div>

<div class={adminPageClass}>
	<!-- 場所・名前・権限を区画に分け、コンテンツの編集ページと同じ石垣に並べる (→ docs/ui.md「編集ページの区画」)。 -->
	<div class={sectionGridClass} {@attach masonry}>
		<section aria-labelledby="location-section-heading" class={sectionCardClass}>
			<h2 id="location-section-heading" class="text-base font-semibold">
				{m.admin_roots_edit_location_title()}
			</h2>
			<!-- パスは区切り以外に折り返せる場所が無いので、どこでも折り返す。 -->
			<p class="text-sm wrap-anywhere" data-testid="root-path">{root.path}</p>
			{#if canPick}
				<div class="flex flex-col items-start gap-2">
					<LoadingButton variant="outline" loading={picking} disabled={moving} onclick={handlePick}
						>{m.admin_roots_edit_repick_button()}</LoadingButton
					>
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
			<!-- 区画の欄は名前だけなので、見出しを欄のラベルにして同じ語を2度出さない。 -->
			<h2 id="name-section-heading">
				<Field.FieldLabel for="root-name" required class="text-base font-semibold"
					>{m.admin_roots_name_label()}</Field.FieldLabel
				>
			</h2>
			<Input id="root-name" autocomplete="off" bind:value={name} required />
			<div>
				<LoadingButton
					type="submit"
					loading={sections.savingPart === 'name'}
					disabled={sections.saving || !nameForm.dirty || name.trim() === ''}
					>{m.action_save()}</LoadingButton
				>
			</div>
		</form>

		<!-- 管理者はどの共有でもすべての操作ができるので、選べるのは一般ユーザーの権限だけ (→ docs/shares.md「できること」)。 -->
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
			</Field.Field>
			<div class="flex flex-col gap-2">
				<p class="text-sm font-medium">{m.admin_roots_access_users_label()}</p>
				{#if exceptionRows.length === 0 && candidates.length > 0}
					<p class="text-sm text-muted-foreground">{m.admin_roots_access_no_exceptions()}</p>
				{:else if exceptionRows.length > 0}
					<ul class="flex flex-col">
						{#each exceptionRows as exception (exception.userId)}
							<li class={[listItemClass, 'flex min-h-11 items-center gap-3 py-1']}>
								<span class="min-w-0 flex-1 text-sm wrap-anywhere">{exception.username}</span>
								<Select.Root
									type="single"
									bind:value={
										() => exception.access, (value) => setExceptionAccess(exception, value)
									}
								>
									<Select.Trigger
										class="w-40 shrink-0"
										aria-label={m.admin_roots_access_user_select({
											username: exception.username,
											access: shareAccessLabel(exception.access)
										})}
									>
										{shareAccessLabel(exception.access)}
									</Select.Trigger>
									<Select.Content>
										{#each ACCESS_CHOICES as access (access)}
											<Select.Item value={access}>{shareAccessLabel(access)}</Select.Item>
										{/each}
									</Select.Content>
								</Select.Root>
								<Button
									type="button"
									variant="ghost"
									size="icon"
									class="-mx-1.5 size-11 shrink-0 text-muted-foreground"
									aria-label={m.admin_roots_access_remove({ username: exception.username })}
									onclick={() => removeException(exception)}
								>
									<XIcon class="size-4" />
								</Button>
							</li>
						{/each}
					</ul>
				{/if}
				<!-- 候補を選ぶと行が増え、権限はその行で選ぶ。足せる人がいなければ出さない。 -->
				{#if candidates.length > 0}
					<DropdownMenu.Root>
						<DropdownMenu.Trigger
							class={[buttonVariants({ variant: 'outline' }), 'mt-2 self-start']}
						>
							{m.admin_roots_access_add_button()}
						</DropdownMenu.Trigger>
						<DropdownMenu.Content align="start">
							{#each candidates as user (user.id)}
								<DropdownMenu.Item
									onSelect={() => (exceptions = addException(exceptions, user, editorAccess))}
									>{user.username}</DropdownMenu.Item
								>
							{/each}
						</DropdownMenu.Content>
					</DropdownMenu.Root>
				{/if}
				{#if data.users === null}
					<p class="text-sm text-muted-foreground">{m.admin_roots_access_users_failed()}</p>
				{:else if candidates.length === 0 && exceptionRows.length === 0}
					<p class="text-sm text-muted-foreground">{m.admin_roots_access_no_candidates()}</p>
				{/if}
			</div>
			<div>
				<LoadingButton
					type="submit"
					loading={sections.savingPart === 'access'}
					disabled={sections.saving || !accessForm.dirty}>{m.action_save()}</LoadingButton
				>
			</div>
		</form>
	</div>
	<DeleteSection
		notes={[m.admin_roots_delete_section_note()]}
		onclick={() => (deletingRoot = root)}
	/>
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
	ondeleted={() => gotoWithoutLeaveConfirm(resolve('admin/roots'))}
/>

<LeaveConfirmDialog guard={sections.leaveGuard} ondiscard={() => sections.discard()} />

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
