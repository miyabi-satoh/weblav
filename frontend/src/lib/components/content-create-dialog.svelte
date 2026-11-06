<script lang="ts">
	/**
	 * コンテンツを追加の窓。管理画面の一覧と、閲覧の画面 (ホーム・グループ) の「追加」で使う。
	 * 種別を選ぶ段と必須の項目を入れる段の2段で、作ったら編集ページへ移り、任意の項目を続けて入れる
	 * (→ docs/ui.md「UI 全般」)。開くのは `start()`。
	 */
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { dialogFormClass } from '$lib/dialog-form';
	import { client } from '$lib/api/client';
	import { isAdmin } from '$lib/auth';
	import { contentTypeLabel } from '$lib/content-labels';
	import {
		ContentFormState,
		contentFormEffectiveVisibility,
		contentFormVisibilityNote
	} from '$lib/content-form.svelte';
	import { byIdMap } from '$lib/content-tree';
	import { isDirectoryContentType, isPathContentType } from '$lib/content-types';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { selectableVisibilities } from '$lib/visibility';
	import { parentPath } from '$lib/path-trail';
	import * as m from '$lib/paraglide/messages.js';
	import type { components } from '$lib/api/schema';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import ConfirmRegister from '$lib/components/confirm-register.svelte';
	import ContentForm from '$lib/components/content-form.svelte';
	import ContentTypeChooser from '$lib/components/content-type-chooser.svelte';
	import DirPicker from '$lib/components/dir-picker.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';

	type Content = components['schemas']['AdminContentResponse'];
	type ContentType = components['schemas']['ContentType'];

	type Props = {
		/** 管理画面の一覧の行。親グループの公開範囲を重ねた見え方を出すのに使う。 */
		contents: Content[];
		/** 「共有」が1件でもあるか (→ `fetchSharedFolderPresence`)。 */
		hasSharedFolders: boolean;
	};

	let { contents, hasSharedFolders }: Props = $props();

	// 種別による制限は持たない。ログイン済みなら全部作れる (→ docs/access.md「ロールと操作」)。
	// ファイルは共有の中に置くので、ここでは作らない。前から登録してある file コンテンツは一覧に残る。
	// `folder` の行はショートカットとして出す。ピッカーでファイルを選ぶと `shortcut` になる。ライブラリはここに出さない。
	const CREATABLE_TYPES: ContentType[] = ['link', 'folder', 'group'];

	// 「共有」が0件なら、パスを持つ種別は選ばせない (→ docs/folders.md「共有」)。
	let unavailableTypes = $derived(hasSharedFolders ? [] : (['folder'] as ContentType[]));
	// 登録できるのはサーバーのパソコンの前にいる管理者だけなので、編集者には頼み先を示す。
	let unavailableNote = $derived(
		isAdmin(page.data.user)
			? m.contents_create_type_needs_shared_folder_note()
			: m.contents_create_type_needs_shared_folder_note_user()
	);
	let contentById = $derived(byIdMap(contents));

	let open = $state(false);
	let createStep = $state<'type' | 'form'>('type');
	// 種別を「追加」のメニューで選んで開いたか。そのときは種別を選ぶ段へ戻さない。
	let typeChosenBeforeOpen = $state(false);
	// 「この中に追加...」・グループの画面の「追加」で開いたときの親グループ。
	let createParent = $state<Content | null>(null);
	const form = new ContentFormState();
	let saving = $state(false);
	let dirPickerOpen = $state(false);
	let confirmRegisterOpen = $state(false);
	const errorDialog = new ErrorDialogState();

	let visibilityOptions = $derived(selectableVisibilities(null, createParent));
	let visibilityNote = $derived(contentFormVisibilityNote(form, contentById));
	let effectiveFormVisibility = $derived(contentFormEffectiveVisibility(form, contentById));

	// フォルダーを指すものの新規作成では、登録前の確認を常に出す。ファイルを指すものは、数えるものが無いので出さない
	// (見える範囲は追加の窓の公開範囲の欄で分かる)。
	let needsRegisterConfirm = $derived(isDirectoryContentType(form.type) && form.path !== '');

	/**
	 * 窓を開く。`type` を渡すと入力の段から始める。選べない種別 (共有が無いときのショートカット) なら、
	 * 理由が見えるよう種別を選ぶ段から始める。
	 */
	export function start(parent: Content | null, type: ContentType | null = null) {
		form.resetForCreate(parent === null ? 'root' : String(parent.id));
		createParent = parent;
		typeChosenBeforeOpen = type !== null && !unavailableTypes.includes(type);
		if (typeChosenBeforeOpen && type !== null) {
			chooseType(type);
		} else {
			createStep = 'type';
		}
		open = true;
	}

	function chooseType(type: ContentType) {
		// 種別を選んだだけでは入力途中として扱わない。外側のクリックで閉じられるようにするため。
		const wasDirty = form.dirty;
		form.chooseType(type);
		// 公開範囲は場所を選ぶ種類の2段目にしか出ない。戻って別の種別を選んだときに、見えない値を残さない。
		if (type !== 'folder') form.visibility = 'public';
		// 「作成者のみ」のグループの中には「作成者のみ」しか置けない (選択肢もそれだけ)。
		if (createParent?.visibility === 'private') form.visibility = 'private';
		if (!wasDirty) form.markPristine();
		createStep = 'form';
	}

	// 入力した値は消さない。種別を選び直しても入力し直さずに済むようにするため。
	// 送るのは選んだ種別の項目だけなので (`createRequestFields`)、前の種別の値は送られない。
	function backToTypes() {
		createStep = 'type';
	}

	function handleSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (saving) return;
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
			const result = await errorDialog.attempt(() =>
				client.POST('/api/v1/contents', {
					body: { type: form.type, ...form.createRequestFields() }
				})
			);
			if (result?.data) {
				open = false;
				// 任意の項目は、作った直後に編集ページで続けて入力する。
				await goto(resolve('/contents/[id]', { id: String(result.data.id) }));
			}
		} finally {
			saving = false;
			// 成否によらず確認ダイアログは閉じる。失敗時はフォームが残るので、
			// エラー内容 (選べないパス等) を直して選び直せる。
			confirmRegisterOpen = false;
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content
		interactOutsideBehavior={form.dirty ? 'ignore' : 'close'}
		escapeKeydownBehavior={form.dirty ? 'ignore' : 'close'}
	>
		<Dialog.Header>
			<Dialog.Title>
				{createStep === 'type'
					? m.contents_form_create_title()
					: m.contents_create_form_title({ type: contentTypeLabel(form.type) })}
			</Dialog.Title>
			{#if createParent !== null}
				<Dialog.Description>
					{m.contents_create_parent_note({ group: createParent.title })}
				</Dialog.Description>
			{/if}
		</Dialog.Header>
		{#if createStep === 'type'}
			<Dialog.Body>
				<ContentTypeChooser
					types={CREATABLE_TYPES}
					label={m.contents_create_type_label()}
					disabledTypes={unavailableTypes}
					disabledReason={m.contents_create_type_needs_shared_folder()}
					disabledNote={unavailableNote}
					onselect={chooseType}
				/>
			</Dialog.Body>
		{:else}
			<form onsubmit={handleSubmit} class={dialogFormClass}>
				<Dialog.Body>
					<ContentForm
						{form}
						variant="create"
						{visibilityOptions}
						{visibilityNote}
						onchoosepath={() => (dirPickerOpen = true)}
					/>
				</Dialog.Body>
				<Dialog.Footer>
					{#if typeChosenBeforeOpen}
						<Button type="button" variant="outline" onclick={() => (open = false)}
							>{m.action_cancel()}</Button
						>
					{:else}
						<Button type="button" variant="outline" onclick={backToTypes}
							>{m.contents_create_back_button()}</Button
						>
					{/if}
					<!-- パスはテキスト入力ではないため、required では未選択を防げない。 -->
					<LoadingButton
						type="submit"
						loading={saving}
						disabled={isPathContentType(form.type) && form.path === ''}
						>{m.action_add()}</LoadingButton
					>
				</Dialog.Footer>
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>

<DirPicker
	bind:open={dirPickerOpen}
	initialPath={form.pathIsFile ? parentPath(form.path) : form.path}
	mode="any"
	onselect={(path, label, isFile) => form.choosePath(path, label, isFile)}
/>

<ConfirmRegister
	bind:open={confirmRegisterOpen}
	contentType={form.type}
	path={form.path}
	pathLabel={form.pathLabel}
	visibility={effectiveFormVisibility}
	{saving}
	onconfirm={saveContent}
/>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
