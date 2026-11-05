<script lang="ts">
	import { LeaveGuard } from '$lib/leave-guard.svelte';
	import { adminPageClass, sectionCardClass, sectionGridClass } from '$lib/page-layout';
	import { masonry } from '$lib/masonry';
	import { untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { refreshAll } from '$app/navigation';
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
	import { cancelScan, rescanArchive } from '$lib/archive-scan';
	import ScanProgress from '$lib/components/scan-progress.svelte';
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
	import LoadingLabel from '$lib/components/loading-label.svelte';
	import ConfirmRegister from '$lib/components/confirm-register.svelte';
	import ContentForm from '$lib/components/content-form.svelte';
	import * as Field from '$lib/components/ui/field';
	import * as Select from '$lib/components/ui/select';
	import DirPicker from '$lib/components/dir-picker.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import LeaveConfirmDialog from '$lib/components/leave-confirm-dialog.svelte';
	import type { components } from '$lib/api/schema';
	import type { PageProps } from './$types';

	type Visibility = components['schemas']['Visibility'];

	let { data }: PageProps = $props();

	let content = $derived(data.content);
	let contentById = $derived(byIdMap(data.contents));
	let canSeeFullPath = $derived(isAdmin(page.data.user));
	let editable = $derived(data.editable);

	// 別のコンテンツへ移るとレイアウトの `{#key}` でページごと作り直すので、読み込みは初回だけでよい。
	const form = new ContentFormState();
	form.loadFrom(
		untrack(() => data.content),
		untrack(() => canSeeFullPath)
	);

	// 作成者の付け替えは admin だけ (→ docs/access.md「ロールと操作」)。
	// 基本情報の区画の保存ボタンで一緒に保存する。
	let canChangeCreator = $derived(isAdmin(page.data.user));
	let creatorId = $state(untrack(() => String(data.content.createdBy ?? '')));
	const creatorDirty = new FormDirtyState(() => creatorId);
	creatorDirty.markPristine();
	let creatorChanged = $derived(canChangeCreator && creatorDirty.dirty);

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
	/** 確認の窓に見せる、広がる見え方。保存すると `loosened` は消えるので、閉じ終わるまで出せるよう開いたときの値を持つ。 */
	let loosenedNotice = $state<typeof loosened>(null);
	let confirmVisibilityOpen = $state(false);
	/** 保存すると見えるファイルがどう変わるか。数えられなかったときは `null`。 */
	let visibilityChange = $state<VisibilityPreview | null>(null);

	type VisibilityPreview = components['schemas']['VisibilityPreview'];

	const errorDialog = new ErrorDialogState();

	let visibilityOptions = $derived(
		selectableVisibilities(
			content,
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
				loosenedNotice = loosened;
				confirmLoosenedOpen = true;
				return;
			}
		}
		confirmRegisterOrSave();
	}

	function confirmRegisterOrSave() {
		// 対象ファイル数と実効的な閲覧範囲を見せてから送る (→ docs/folders.md「登録前の確認」)。
		if (needsRegisterConfirm(confirmingPart)) {
			confirmLoosenedOpen = false;
			confirmRegisterOpen = true;
			return;
		}
		void confirmVisibilityOrSave();
	}

	/** 保存でパターンが変わるか。場所を変えるときは、登録の確認に任せる。 */
	function changesVisibleFiles(): boolean {
		if (confirmingPart !== 'specific' || content.type !== 'folder') return false;
		if (content.path !== form.path) return false;
		return (
			(content.includePatterns ?? null) !== form.normalizedIncludePatterns ||
			(content.excludePatterns ?? null) !== form.normalizedExcludePatterns
		);
	}

	// 見えるファイルが変わるときだけ、保存の前に知らせる (→ docs/folders.md「見せるものの絞り込み」)。
	async function confirmVisibilityOrSave() {
		if (saving) return;
		if (!changesVisibleFiles()) {
			void saveContent();
			return;
		}
		savingPart = confirmingPart;
		let preview: VisibilityPreview | null = null;
		try {
			const result = await client.POST('/api/v1/contents/{id}/visibility-preview', {
				params: { path: { id: content.id } },
				body: {
					includePatterns: form.normalizedIncludePatterns,
					excludePatterns: form.normalizedExcludePatterns
				}
			});
			// 正しくないパターンは保存もできないので、知らせる窓を挟まずに直してもらう。
			if (result.response.status === 422) {
				errorDialog.show(errorMessage(result.error));
				await resumeNavigationAfterSave();
				return;
			}
			preview = result.data ?? null;
		} catch {
			// 数えられなくても保存は止めず、件数なしで知らせる。
		} finally {
			savingPart = null;
		}
		visibilityChange = preview;
		// 一部だけを数えたときは、増減が0でも、数えていない所で変わるかもしれない。
		if (preview !== null && !preview.truncated && preview.gained === 0 && preview.lost === 0) {
			void saveContent();
			return;
		}
		// 数える間に止めた移動は持ち越さない。確認で取り消した後の保存で、急に移ってしまうため。
		await resumeNavigationAfterSave();
		confirmVisibilityOpen = true;
	}

	/** 確認の窓を閉じる。処理中は開いたままにし、応答を受けて成否によらず閉じる (→ docs/ui.md「UI 全般」)。 */
	function closeConfirms() {
		confirmLoosenedOpen = false;
		confirmVisibilityOpen = false;
		confirmRegisterOpen = false;
	}

	async function saveCreator(): Promise<boolean> {
		const result = await client.PUT('/api/v1/contents/{id}/creator', {
			params: { path: { id: content.id } },
			body: { createdBy: Number(creatorId) }
		});
		if (!result.response.ok) {
			closeConfirms();
			errorDialog.show(errorMessage(result.error));
			return false;
		}
		creatorDirty.markPristine();
		return true;
	}

	async function saveContent() {
		if (saving) return;
		const part = confirmingPart;
		savingPart = part;
		// 知らせには、押した保存ボタンの区画の見出しを出す。
		const section = (part === 'common' ? m.contents_edit_basic_title() : specificTitle) ?? '';

		try {
			// 場所を変えると索引が空になり、一覧の表示を使い始めたときは索引がまだ無いので、
			// 保存に続けてスキャンする (→ docs/archive.md「スキャン」)。パターンで見えるファイルが増えるときも、
			// 増えた分が索引に無いので同じ (→ docs/folders.md「見せるものの絞り込み」)。
			const rescansAfterSave =
				part === 'specific' &&
				form.partDirty('specific') &&
				content.type === 'folder' &&
				form.listView &&
				(content.path !== form.path ||
					!content.listView ||
					(changesVisibleFiles() &&
						(visibilityChange === null ||
							visibilityChange.truncated ||
							visibilityChange.gained > 0)));
			// 「作成者のみ」のグループは中身と作成者が揃っている必要があるので、そこから外すときは公開範囲を先に保存する。
			const creatorLast = content.visibility === 'private' && form.visibility !== 'private';
			if (part === 'common' && creatorChanged && !creatorLast && !(await saveCreator())) return;
			if (form.partDirty(part)) {
				const result = await form.replacePart(content.id, part, content);
				if (!result.response.ok || !result.data) {
					closeConfirms();
					errorDialog.show(errorMessage(result.error));
					// 作成者だけ保存できていることがあるので、見出しと一覧は取り直す。
					await refreshAll();
					return;
				}
				form.loadPart(result.data, canSeeFullPath, part);
			}
			if (part === 'common' && creatorChanged && creatorLast && !(await saveCreator())) {
				// 公開範囲は保存できているので、見出しと一覧は取り直す。
				await refreshAll();
				return;
			}
			if (rescansAfterSave) {
				// 続くスキャンの進み具合と中止は、登録の確認の窓か、区画の中に出す。
				confirmVisibilityOpen = false;
				const scan = await rescanArchive(content.id, (message) => (scanProgress = message));
				closeConfirms();
				if (scan.ok) {
					toast.success(
						scan.result.cancelled
							? m.contents_edit_saved_scan_cancelled_toast({ section })
							: m.contents_edit_saved_scanned_toast({
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
				closeConfirms();
				toast.success(m.common_section_saved_toast({ section }));
			}
			// 見出しのタイトルと、公開範囲の判定に使う一覧を取り直す。
			await refreshAll();
		} catch {
			closeConfirms();
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			savingPart = null;
			closeConfirms();
			await resumeNavigationAfterSave();
		}
	}

	// 保存していない変更を、別の画面へ移って失わないようにする。
	const leaveGuard = new LeaveGuard(
		() => dirty,
		() => saving
	);

	async function resumeNavigationAfterSave() {
		// 保存に失敗して変更が残っていれば移らない。エラーダイアログに確認を重ねないため。
		// 保存はできてもスキャンだけが失敗したときも、再スキャンの案内を読ませるために留まる。
		await leaveGuard.resumeAfterSave(() => dirty || errorDialog.open);
	}

	function discardChanges() {
		form.markPristine();
		creatorDirty.markPristine();
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
			<Field.Field>
				<Field.FieldLabel for="content-creator">{m.contents_form_creator_label()}</Field.FieldLabel>
				<Select.Root type="single" bind:value={creatorId}>
					<Select.Trigger id="content-creator" class="w-full">
						{creatorSelectLabel(creatorId)}
					</Select.Trigger>
					<Select.Content>
						{#each data.users as user (user.id)}
							<Select.Item value={String(user.id)}>{user.username}</Select.Item>
						{/each}
					</Select.Content>
				</Select.Root>
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
					<ScanProgress message={scanProgress} oncancel={() => cancelScan(content.id)} />
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
	includePatterns={(confirmSpecific ? form.normalizedIncludePatterns : content.includePatterns) ??
		undefined}
	excludePatterns={(confirmSpecific ? form.normalizedExcludePatterns : content.excludePatterns) ??
		undefined}
	listView={confirmSpecific ? form.listView : content.listView}
	editing
	{saving}
	progress={scanProgress}
	oncancelscan={() => cancelScan(content.id)}
	onconfirm={saveContent}
/>

<AlertDialog.Root bind:open={confirmLoosenedOpen}>
	<AlertDialog.Content escapeKeydownBehavior={saving ? 'ignore' : 'close'}>
		<AlertDialog.Header>
			<AlertDialog.Title>{m.contents_edit_loosened_title()}</AlertDialog.Title>
			<AlertDialog.Description>
				{#if loosenedNotice !== null}
					{m.contents_edit_loosened_description({
						before: visibilityLabel(loosenedNotice.before),
						after: visibilityLabel(loosenedNotice.after)
					})}
				{/if}
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel disabled={saving}>{m.action_cancel()}</AlertDialog.Cancel>
			<AlertDialog.Action class="relative" disabled={saving} onclick={confirmRegisterOrSave}>
				<LoadingLabel loading={saving}>{m.action_save()}</LoadingLabel>
			</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<AlertDialog.Root bind:open={confirmVisibilityOpen}>
	<AlertDialog.Content escapeKeydownBehavior={saving ? 'ignore' : 'close'}>
		<AlertDialog.Header>
			<AlertDialog.Title>{m.contents_edit_visibility_change_title()}</AlertDialog.Title>
			<AlertDialog.Description>
				{#if visibilityChange === null}
					{m.contents_edit_visibility_change_unknown()}
				{:else}
					{#if visibilityChange.gained > 0}
						<span class="block">
							{m.contents_edit_visibility_change_gained({
								count: formatNumber(visibilityChange.gained)
							})}
						</span>
					{/if}
					{#if visibilityChange.lost > 0}
						<span class="block">
							{m.contents_edit_visibility_change_lost({
								count: formatNumber(visibilityChange.lost)
							})}
						</span>
					{/if}
					{#if visibilityChange.truncated}
						<span class="mt-2 block">{m.contents_edit_visibility_change_truncated()}</span>
					{/if}
				{/if}
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel disabled={saving}>{m.action_cancel()}</AlertDialog.Cancel>
			<AlertDialog.Action class="relative" disabled={saving} onclick={saveContent}>
				<LoadingLabel loading={saving}>{m.action_save()}</LoadingLabel>
			</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<LeaveConfirmDialog guard={leaveGuard} ondiscard={discardChanges} />

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
