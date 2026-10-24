<script lang="ts">
	import { leaveConfirmBypassed } from '$lib/leave-confirm';
	import { adminPageClass, sectionCardClass, sectionGridClass } from '$lib/page-layout';
	import { masonry } from '$lib/masonry';
	import { untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { beforeNavigate, goto, refreshAll } from '$app/navigation';
	import { page } from '$app/state';
	import { isAdmin } from '$lib/auth';
	import { client } from '$lib/api/client';
	import { FormDirtyState } from '$lib/form-dirty.svelte';
	import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
	import {
		type ContentFormPart,
		ContentFormState,
		contentFormEffectiveVisibility,
		contentFormVisibilityError,
		contentFormVisibilityNote
	} from '$lib/content-form.svelte';
	import { ancestorPath, byIdMap, selfAndAncestors } from '$lib/content-tree';
	import { isDirectoryContentType } from '$lib/content-types';
	import { rescanArchive } from '$lib/archive-scan';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import {
		canHoldContentOf,
		effectiveVisibilityUnder,
		loosenedByReparent,
		selectableVisibilities
	} from '$lib/visibility';
	import { contentPathLabel } from '$lib/root-location';
	import { visibilityLabel } from '$lib/content-labels';
	import { formatNumber } from '$lib/format';
	import * as m from '$lib/paraglide/messages.js';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import ConfirmRegister from '$lib/components/confirm-register.svelte';
	import ContentForm from '$lib/components/content-form.svelte';
	import * as Field from '$lib/components/ui/field';
	import * as Select from '$lib/components/ui/select';
	import FolderMarksSection from '$lib/components/folder-marks-section.svelte';
	import DirPicker from '$lib/components/dir-picker.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import type { components } from '$lib/api/schema';
	import type { PageProps } from './$types';

	type Visibility = components['schemas']['Visibility'];

	let { data }: PageProps = $props();

	let content = $derived(data.content);
	let contentById = $derived(byIdMap(data.contents));
	let currentUserId = $derived(page.data.user?.id);
	let canSeeFullPath = $derived(isAdmin(page.data.user));
	let editable = $derived(data.editable);

	// 別のコンテンツへ移るとレイアウトの `{#key}` でページごと作り直すので、読み込みは初回だけでよい。
	const form = new ContentFormState();
	form.loadFrom(
		untrack(() => data.content),
		untrack(() => canSeeFullPath)
	);

	// 作成者の付け替えは admin だけ。`private` は付け替えられない (→ docs/access.md「ロールと操作」)。
	// 基本情報の区画の保存ボタンで一緒に保存する。
	let canChangeCreator = $derived(isAdmin(page.data.user));
	let creatorLocked = $derived(content.visibility === 'private' || form.visibility === 'private');
	let savedCreatorId = $derived(String(content.createdBy ?? ''));
	let creatorId = $state(untrack(() => String(data.content.createdBy ?? '')));
	const creatorDirty = new FormDirtyState(() => creatorId);
	creatorDirty.markPristine();
	let creatorChanged = $derived(canChangeCreator && !creatorLocked && creatorDirty.dirty);

	// 公開・非公開を変えたものが無ければ区画を出さない。印を付けるのは閲覧の画面とアイテムのタブで行う。
	// 一覧の表示を使うフォルダーでは、ファイルの印はアイテムのタブで見るので、フォルダーの印だけを並べる
	// (選んで公開するフォルダーでは、公開したファイルの印が数万件になりうるため)。
	let folderMarks = $derived.by(() => {
		if (content.type !== 'folder' || !data.folderMarks) return null;
		const marks = content.listView
			? data.folderMarks.filter((mark) => mark.isDir)
			: data.folderMarks;
		return marks.length > 0 ? marks : null;
	});

	// 区画ごとに保存する (→ docs/ui.md「編集ページの区画」)。保存ボタンは、その区画の変更だけで決める。
	let commonDirty = $derived(form.partDirty('common') || creatorChanged);
	let specificDirty = $derived(form.partDirty('specific'));
	// 離れるときの確認は、どの区画の変更でも出す。
	let dirty = $derived(commonDirty || specificDirty);

	// 種類ごとの項目の区画の見出し。グループには種類ごとの項目が無い。
	let specificTitle = $derived.by(() => {
		if (content.type === 'link') return m.contents_edit_link_title();
		if (content.type === 'file') return m.contents_edit_file_title();
		if (content.type === 'folder') return m.contents_edit_folder_title();
		return null;
	});

	function partDirty(part: ContentFormPart): boolean {
		return part === 'common' ? commonDirty : specificDirty;
	}

	function creatorSelectLabel(value: string): string {
		if (value === '') return m.contents_form_creator_none();
		return (
			data.users.find((user) => String(user.id) === value)?.username ??
			content.createdByUsername ??
			''
		);
	}

	/** 保存している区画。保存中はどの区画の保存ボタンも押せなくする。 */
	let savingPart = $state<ContentFormPart | null>(null);
	let saving = $derived(savingPart !== null);
	/** 確認の窓を開いている保存の区画。確認の窓には、その区画の項目だけを変えた後の値を見せる。 */
	let confirmingPart = $state<ContentFormPart>('common');
	/** 保存に続くスキャンの進み具合。確認の窓の、押したボタンの近くに出す。 */
	let scanProgress = $state<string | undefined>();

	let dirPickerOpen = $state(false);
	let confirmRegisterOpen = $state(false);
	let confirmLoosenedOpen = $state(false);

	const errorDialog = new ErrorDialogState();

	let visibilityOptions = $derived(
		selectableVisibilities(
			content,
			currentUserId,
			form.parentIdValue == null ? null : contentById.get(form.parentIdValue)
		)
	);
	// 「作成者のみ」のグループを親に選ぶと、中に置けるのは「作成者のみ」だけになるので、それにする
	// (→ docs/access.md「作成者のみのグループ」)。選んだ欄のすぐ下に出るので、黙って変わることにはならない。
	// 親を選び直してそのグループの外へ出したら、切り替える前の公開範囲に戻す。
	// 戻すのは、切り替えたときと同じコンテンツの同じ保存済みの状態のまま編集を続けているときだけ。
	// 保存した後や別のコンテンツへ移った後に、前の値を持ち込まないため。
	let beforePrivateParent: { id: number; saved: Visibility; visibility: Visibility } | null = null;
	$effect(() => {
		if (!visibilityOptions.includes(form.visibility)) {
			beforePrivateParent = {
				id: content.id,
				saved: content.visibility,
				visibility: form.visibility
			};
			form.visibility = visibilityOptions[0];
		} else if (beforePrivateParent !== null && visibilityOptions.length > 1) {
			const { id, saved, visibility } = beforePrivateParent;
			beforePrivateParent = null;
			if (
				id === content.id &&
				saved === content.visibility &&
				visibilityOptions.includes(visibility)
			)
				form.visibility = visibility;
		}
	});
	let visibilityError = $derived(contentFormVisibilityError(form, content, data.contents));
	let visibilityNote = $derived(contentFormVisibilityNote(form, contentById));
	let effectiveFormVisibility = $derived(contentFormEffectiveVisibility(form, contentById));

	// パスか公開範囲が変わったときだけ確認を出す。タイトルだけの変更で巨大なツリーを数え直さないため。
	// パスは種類ごとの区画、公開範囲は基本情報の区画にあるので、保存する区画の側の変更だけを見る。
	function needsRegisterConfirm(part: ContentFormPart): boolean {
		if (!isDirectoryContentType(form.type) || form.path === '') return false;
		return part === 'specific'
			? content.path !== form.path
			: content.visibility !== form.visibility;
	}

	// 確認の窓に見せる値。保存しない区画の項目は保存済みの値にする。
	let confirmSpecific = $derived(confirmingPart === 'specific');
	let confirmVisibility = $derived(
		confirmSpecific
			? effectiveVisibilityUnder(content.visibility, content.parentId, contentById)
			: effectiveFormVisibility
	);
	let confirmPathLabel = $derived(
		confirmSpecific ? form.pathLabel : (contentPathLabel(content, canSeeFullPath) ?? '')
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
	// (サーバーの `validate_parent` も拒む)。「作成者のみ」のグループは、その作成者のものの親にだけなれる。
	let parentCandidates = $derived(
		data.contents
			.filter((c) => c.type === 'group' && canHoldContentOf(c, content.createdBy))
			.filter((c) => !selfAndAncestors(c.id, contentById).some((a) => a.id === content.id))
			.map((c) => ({ id: c.id, label: ancestorPath(c.id, contentById) }))
	);

	function handleSubmit(e: SubmitEvent, part: ContentFormPart) {
		e.preventDefault();
		if (saving) return;
		confirmingPart = part;
		if (part === 'common') {
			if (visibilityError !== null) return;
			if (loosened !== null) {
				confirmLoosenedOpen = true;
				return;
			}
		}
		confirmRegisterOrSave();
	}

	function confirmRegisterOrSave() {
		confirmLoosenedOpen = false;
		// 対象ファイル数と実効的な閲覧範囲を見せてから送る (→ docs/folders.md「登録前の確認」)。
		if (needsRegisterConfirm(confirmingPart)) {
			confirmRegisterOpen = true;
			return;
		}
		saveContent();
	}

	async function saveContent() {
		if (saving) return;
		const part = confirmingPart;
		savingPart = part;
		// 知らせには、押した保存ボタンの区画の見出しを出す。
		const section = (part === 'common' ? m.contents_edit_basic_title() : specificTitle) ?? '';

		try {
			// 場所を変えると索引が空になり、一覧の表示を使い始めたときは索引がまだ無いので、
			// 保存に続けてスキャンする (→ docs/archive.md「スキャン」)。
			const rescansAfterSave =
				part === 'specific' &&
				form.partDirty('specific') &&
				content.type === 'folder' &&
				form.listView &&
				(content.path !== form.path || !content.listView);
			if (part === 'common' && creatorChanged) {
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
			if (form.partDirty(part)) {
				const result = await form.replacePart(content.id, part, content);
				if (!result.response.ok || !result.data) {
					errorDialog.show(errorMessage(result.error));
					// 作成者だけ保存できていることがあるので、見出しと一覧は取り直す。
					await refreshAll();
					return;
				}
				form.loadPart(result.data, canSeeFullPath, part);
			}
			if (rescansAfterSave) {
				const scan = await rescanArchive(content.id, (message) => (scanProgress = message));
				if (scan.ok) {
					toast.success(
						m.contents_edit_saved_scanned_toast({
							section,
							total: formatNumber(scan.result.total)
						})
					);
				} else {
					errorDialog.show(
						scan.retryable
							? m.contents_edit_scan_failed({ section, reason: scan.message })
							: m.contents_edit_scan_blocked({ section, reason: scan.message }),
						m.archive_scan_failed_title()
					);
				}
			} else {
				toast.success(m.contents_edit_saved_toast({ section }));
			}
			// 見出しのタイトルと、公開範囲の判定に使う一覧を取り直す。
			await refreshAll();
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			savingPart = null;
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
		if (leaveConfirmBypassed()) return;
		// タブを閉じる・再読み込みは、ブラウザー標準の確認に任せる。
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

{#snippet partForm(part: ContentFormPart, title: string)}
	<form
		onsubmit={(e) => handleSubmit(e, part)}
		aria-labelledby="{part}-section-heading"
		class={sectionCardClass}
	>
		<h2 id="{part}-section-heading" class="text-base font-semibold">{title}</h2>
		<!-- 見るだけのときは、入力欄を押せない状態で値を見せる (→ docs/ui.md「UI 全般」)。 -->
		<fieldset disabled={!editable} class="contents">
			<ContentForm
				{form}
				variant="edit"
				{part}
				{parentCandidates}
				{visibilityOptions}
				{visibilityError}
				{visibilityNote}
				onchoosepath={() => (dirPickerOpen = true)}
				disabled={!editable}
			/>
		</fieldset>
		{#if part === 'common' && canChangeCreator}
			<Field.Field data-disabled={creatorLocked}>
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
			<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
				<!-- パスはテキスト入力ではないため、required では未選択を防げない。 -->
				<LoadingButton
					type="submit"
					loading={savingPart === part}
					disabled={saving ||
						!partDirty(part) ||
						(part === 'specific' && isDirectoryContentType(form.type) && form.path === '') ||
						(part === 'common' && visibilityError !== null)}>{m.action_save()}</LoadingButton
				>
				<!-- 確認の窓を通らずに始めたスキャン (一覧の表示を使い始めたとき) は、ボタンの横に進み具合を出す。 -->
				{#if part === 'specific' && scanProgress && !confirmRegisterOpen}
					<p role="status" class="text-sm text-muted-foreground">{scanProgress}</p>
				{/if}
			</div>
		{/if}
	</form>
{/snippet}

<div class={adminPageClass}>
	<!-- どの種類も、基本情報と種類ごとの項目を区画に分け、石垣に並べる (→ docs/ui.md「編集ページの区画」)。 -->
	<div class={sectionGridClass} {@attach masonry}>
		{@render partForm('common', m.contents_edit_basic_title())}
		{#if specificTitle !== null}
			{@render partForm('specific', specificTitle)}
		{/if}
		{#if folderMarks != null}
			<FolderMarksSection contentId={content.id} marks={folderMarks} />
		{/if}
	</div>
</div>

<DirPicker
	bind:open={dirPickerOpen}
	initialPath={form.path}
	onselect={(path, label) => form.choosePath(path, label)}
/>

<ConfirmRegister
	bind:open={confirmRegisterOpen}
	contentType={form.type}
	path={confirmSpecific ? form.path : (content.path ?? '')}
	pathLabel={confirmPathLabel}
	visibility={confirmVisibility}
	extensions={(confirmSpecific ? form.normalizedExtensions : content.extensions) ?? undefined}
	listView={confirmSpecific ? form.listView : content.listView}
	newFilesPublic={confirmSpecific ? form.newFilesPublic : content.newFilesPublic}
	clearsMarks={confirmSpecific && content.type === 'folder' && content.path !== form.path}
	editing
	{saving}
	progress={scanProgress}
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
