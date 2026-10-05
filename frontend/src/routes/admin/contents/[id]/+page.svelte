<script lang="ts">
	import { adminPageClass } from '$lib/page-layout';
	import { untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { beforeNavigate, goto, invalidateAll } from '$app/navigation';
	import { page } from '$app/state';
	import { canManageRoots } from '$lib/root-location';
	import { isAdmin } from '$lib/auth';
	import { client } from '$lib/api/client';
	import { FormDirtyState } from '$lib/form-dirty.svelte';
	import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
	import {
		ContentFormState,
		contentFormEffectiveVisibility,
		contentFormVisibilityError,
		contentFormVisibilityNote
	} from '$lib/content-form.svelte';
	import { ancestorPath, byIdMap, selfAndAncestors } from '$lib/content-tree';
	import { isDirectoryContentType } from '$lib/content-types';
	import { rescanArchive } from '$lib/archive-scan';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { loosenedByReparent, selectableVisibilities } from '$lib/visibility';
	import { visibilityLabel } from '$lib/content-labels';
	import { formatNumber } from '$lib/format';
	import * as m from '$lib/paraglide/messages.js';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import ConfirmRegister from '$lib/components/confirm-register.svelte';
	import ContentForm from '$lib/components/content-form.svelte';
	import * as Field from '$lib/components/ui/field';
	import * as Select from '$lib/components/ui/select';
	import DirPicker from '$lib/components/dir-picker.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	let content = $derived(data.content);
	let contentById = $derived(byIdMap(data.contents));
	let currentUserId = $derived(page.data.user?.id);
	let canSeeFullPath = $derived(canManageRoots(page.data.user, page.url.hostname));
	let editable = $derived(data.editable);

	// 別のコンテンツへ移るとレイアウトの `{#key}` でページごと作り直すので、読み込みは初回だけでよい。
	const form = new ContentFormState();
	form.loadFrom(
		untrack(() => data.content),
		untrack(() => canSeeFullPath)
	);

	// 作成者の付け替えは admin だけ。`private` は付け替えられない (→ docs/access.md「ロールと操作」)。
	// 基本情報の保存ボタンで一緒に保存する。
	let canChangeCreator = $derived(isAdmin(page.data.user));
	let creatorLocked = $derived(content.visibility === 'private' || form.visibility === 'private');
	let savedCreatorId = $derived(String(content.createdBy ?? ''));
	let creatorId = $state(untrack(() => String(data.content.createdBy ?? '')));
	const creatorDirty = new FormDirtyState(() => creatorId);
	creatorDirty.markPristine();
	let creatorChanged = $derived(canChangeCreator && !creatorLocked && creatorDirty.dirty);
	let dirty = $derived(form.dirty || creatorChanged);

	function creatorSelectLabel(value: string): string {
		if (value === '') return m.contents_form_creator_none();
		return (
			data.users.find((user) => String(user.id) === value)?.username ??
			content.createdByUsername ??
			''
		);
	}

	let saving = $state(false);

	let dirPickerOpen = $state(false);
	let confirmRegisterOpen = $state(false);
	let confirmLoosenedOpen = $state(false);

	const errorDialog = new ErrorDialogState();

	let visibilityOptions = $derived(selectableVisibilities(form.type, content, currentUserId));
	let visibilityError = $derived(contentFormVisibilityError(form));
	let visibilityNote = $derived(contentFormVisibilityNote(form, contentById));
	let effectiveFormVisibility = $derived(contentFormEffectiveVisibility(form, contentById));

	// パスか公開範囲が変わったときだけ確認を出す。タイトルだけの変更で巨大なツリーを数え直さないため。
	let needsRegisterConfirm = $derived(
		isDirectoryContentType(form.type) &&
			form.path !== '' &&
			(content.path !== form.path || content.visibility !== form.visibility)
	);

	// 親の付け替えで見える範囲が広がるとき。公開範囲は書き換えないので、保存前に知らせる (→ docs/access.md「親が外れるときの公開範囲」)。
	let loosened = $derived(
		loosenedByReparent(
			content,
			{ visibility: form.visibility, parentId: form.parentIdValue },
			contentById
		)
	);

	// 親に指定できるのは group だけ。自分自身と子孫は循環になるので候補に出さない
	// (サーバーの `validate_parent` も拒む)。
	let parentCandidates = $derived(
		data.contents
			.filter((c) => c.type === 'group')
			.filter((c) => !selfAndAncestors(c.id, contentById).some((a) => a.id === content.id))
			.map((c) => ({ id: c.id, label: ancestorPath(c.id, contentById) }))
	);

	function handleSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (saving || visibilityError !== null) return;
		if (loosened !== null) {
			confirmLoosenedOpen = true;
			return;
		}
		confirmRegisterOrSave();
	}

	function confirmRegisterOrSave() {
		confirmLoosenedOpen = false;
		// 対象ファイル数と実効的な閲覧範囲を見せてから送る (→ docs/folders.md「登録前の確認」)。
		if (needsRegisterConfirm) {
			confirmRegisterOpen = true;
			return;
		}
		saveContent();
	}

	async function saveContent() {
		if (saving) return;
		saving = true;

		try {
			// 場所を変えると索引が空になるので、保存に続けてスキャンする (→ docs/archive.md「スキャン」)。
			const rescansAfterSave =
				form.dirty && content.type === 'archive' && content.path !== form.path;
			// 作成者を先に送る。基本情報の保存で場所が変わると索引が空になるので、その後に
			// 失敗して再スキャンまで届かない形を作らないため。
			if (creatorChanged) {
				const result = await client.PUT('/api/v1/contents/{id}/creator', {
					params: { path: { id: content.id } },
					body: { createdBy: Number(creatorId) }
				});
				if (!result.response.ok) {
					errorDialog.show(errorMessage(result.error));
					return;
				}
				creatorDirty.markPristine();
			}
			if (form.dirty) {
				const result = await form.replace(content.id);
				if (!result.response.ok || !result.data) {
					errorDialog.show(errorMessage(result.error));
					// 作成者だけ保存できていることがあるので、見出しと一覧は取り直す。
					await invalidateAll();
					return;
				}
				form.loadFrom(result.data, canSeeFullPath);
			}
			if (rescansAfterSave) {
				const scan = await rescanArchive(content.id);
				if (scan.ok) {
					toast.success(
						m.contents_edit_saved_scanned_toast({ total: formatNumber(scan.result.total) })
					);
				} else {
					errorDialog.show(
						m.contents_edit_scan_failed({ reason: scan.message }),
						m.archive_scan_failed_title()
					);
				}
			} else {
				toast.success(m.contents_edit_saved_toast());
			}
			// 見出しのタイトルと、公開範囲の判定に使う一覧を取り直す。
			await invalidateAll();
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			saving = false;
			// 成否によらず確認ダイアログは閉じる。失敗時はフォームが残るので、直して保存し直せる。
			confirmRegisterOpen = false;
		}
		await resumeNavigationAfterSave();
	}

	// 保存していない変更を、別の画面へ移って失わないようにする。
	let pendingNavigation = $state<URL | null>(null);

	// 保存中に移ろうとした行き先。保存が終わってから、結果に応じて移るか確認を出す。
	let navigationAfterSave: URL | null = null;

	beforeNavigate((navigation) => {
		// 保存は、成功して変更が無くなった後の取り直しまで含めて待つ。
		if (!dirty && !saving) return;
		// タブを閉じる・再読み込みは、ブラウザ標準の確認に任せる。
		if (navigation.type === 'leave') {
			navigation.cancel();
			return;
		}
		if (!navigation.to) return;
		navigation.cancel();
		if (saving) {
			navigationAfterSave = navigation.to.url;
			return;
		}
		pendingNavigation = navigation.to.url;
	});

	async function resumeNavigationAfterSave() {
		const target = navigationAfterSave;
		navigationAfterSave = null;
		// 保存に失敗して変更が残っていれば移らない。エラーダイアログに確認を重ねないため。
		// 保存はできてもスキャンだけが失敗したときも、再スキャンの案内を読ませるために留まる。
		if (target === null || dirty || errorDialog.open) return;
		// 行き先は SvelteKit のナビゲーションが解決済みの URL。
		// eslint-disable-next-line svelte/no-navigation-without-resolve
		await goto(target);
	}

	async function discardAndLeave() {
		const target = pendingNavigation;
		pendingNavigation = null;
		if (target === null) return;
		// 変更を捨てたので、移動をもう一度止めないようにする。
		form.markPristine();
		creatorDirty.markPristine();
		// 行き先は SvelteKit のナビゲーションが解決済みの URL。
		// eslint-disable-next-line svelte/no-navigation-without-resolve
		await goto(target);
	}
</script>

<div class={adminPageClass}>
	<form onsubmit={handleSubmit} class="flex max-w-xl flex-col gap-6">
		<!-- 見るだけのときは、入力欄を押せない状態で値を見せる (→ docs/ui.md「UI 全般」)。 -->
		<fieldset disabled={!editable} class="contents">
			<ContentForm
				{form}
				variant="edit"
				{parentCandidates}
				{visibilityOptions}
				{visibilityError}
				{visibilityNote}
				onchoosepath={() => (dirPickerOpen = true)}
				disabled={!editable}
			/>
		</fieldset>
		{#if canChangeCreator}
			<Field.Field>
				<Field.FieldLabel for="content-creator">{m.contents_form_creator_label()}</Field.FieldLabel>
				<!-- 付け替えられない間は、選んでいた値でなく保存済みの作成者を見せる。送らない値を出さないため。 -->
				<Select.Root
					type="single"
					bind:value={() => (creatorLocked ? savedCreatorId : creatorId), (v) => (creatorId = v)}
					disabled={creatorLocked}
				>
					<Select.Trigger id="content-creator" class="w-full">
						{creatorSelectLabel(creatorLocked ? savedCreatorId : creatorId)}
					</Select.Trigger>
					<Select.Content>
						{#each data.users as user (user.id)}
							<Select.Item value={String(user.id)}>{user.username}</Select.Item>
						{/each}
					</Select.Content>
				</Select.Root>
				{#if creatorLocked}
					<Field.FieldDescription>{m.contents_form_creator_private_note()}</Field.FieldDescription>
				{/if}
			</Field.Field>
		{/if}
		{#if editable}
			<div>
				<!-- パスはテキスト入力ではないため、required では未選択を防げない。 -->
				<LoadingButton
					type="submit"
					loading={saving}
					disabled={!dirty ||
						(isDirectoryContentType(form.type) && form.path === '') ||
						visibilityError !== null}>{m.action_save()}</LoadingButton
				>
			</div>
		{/if}
	</form>
</div>

<DirPicker
	bind:open={dirPickerOpen}
	initialPath={form.path}
	onselect={(path, label) => form.choosePath(path, label)}
/>

<ConfirmRegister
	bind:open={confirmRegisterOpen}
	contentType={form.type}
	path={form.path}
	pathLabel={form.pathLabel}
	visibility={effectiveFormVisibility}
	extensions={form.normalizedExtensions ?? undefined}
	clearsPublished={content.type === 'archive' && content.path !== form.path}
	{saving}
	onconfirm={saveContent}
/>

<AlertDialog.Root bind:open={confirmLoosenedOpen}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{m.contents_edit_loosened_title()}</AlertDialog.Title>
			<AlertDialog.Description>
				{#if loosened !== null}
					{m.contents_edit_loosened_description({
						before: visibilityLabel(loosened.before),
						after: visibilityLabel(loosened.after)
					})}
				{/if}
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>{m.action_cancel()}</AlertDialog.Cancel>
			<!-- 保存の前に閉じるので、保存中の表示は持たない (保存ボタンの側が出す)。 -->
			<AlertDialog.Action onclick={confirmRegisterOrSave}>{m.action_save()}</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<AlertDialog.Root
	bind:open={
		() => pendingNavigation !== null,
		(open) => {
			if (!open) pendingNavigation = null;
		}
	}
>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{m.contents_edit_discard_title()}</AlertDialog.Title>
			<AlertDialog.Description>{m.contents_edit_discard_description()}</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>{m.action_cancel()}</AlertDialog.Cancel>
			<AlertDialog.Action variant="destructive" onclick={discardAndLeave}
				>{m.contents_edit_discard_confirm()}</AlertDialog.Action
			>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
