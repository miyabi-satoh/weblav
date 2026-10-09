<script lang="ts">
	import {
		adminPageClass,
		adminPageHeaderClass,
		pageHeadingTextClass,
		adminTableBandClass
	} from '$lib/page-layout';
	import { goto, invalidateAll } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { client } from '$lib/api/client';
	import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
	import { canEditContent, isAdmin } from '$lib/auth';
	import { contentTypeLabel, visibilityLabel } from '$lib/content-labels';
	import {
		ContentFormState,
		contentFormEffectiveVisibility,
		contentFormVisibilityNote
	} from '$lib/content-form.svelte';
	import { ancestorPath, byIdMap, flattenTree, selfAndAncestors } from '$lib/content-tree';
	import { contentTypeIcon, isDirectoryContentType } from '$lib/content-types';
	import { rescanArchive } from '$lib/archive-scan';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import {
		childrenLoosenedByDeletingGroup,
		loosenedByReparent,
		selectableVisibilities
	} from '$lib/visibility';
	import { ReparentDrag } from '$lib/reparent-drag.svelte';
	import { formatNumber } from '$lib/format';
	import * as m from '$lib/paraglide/messages.js';
	import type { components } from '$lib/api/schema';
	import ArrowUpToLineIcon from '@lucide/svelte/icons/arrow-up-to-line';
	import GripVerticalIcon from '@lucide/svelte/icons/grip-vertical';
	import { Button } from '$lib/components/ui/button';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Table from '$lib/components/ui/table';
	import { canManageRoots, contentPathLabel } from '$lib/root-location';
	import ConfirmRegister from '$lib/components/confirm-register.svelte';
	import ContentForm from '$lib/components/content-form.svelte';
	import ContentTypeChooser from '$lib/components/content-type-chooser.svelte';
	import DirPicker from '$lib/components/dir-picker.svelte';
	import ConfirmDialog from '$lib/components/confirm-dialog.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import LoadingLabel from '$lib/components/loading-label.svelte';
	import RowActionsMenu from '$lib/components/row-actions-menu.svelte';
	import type { PageProps } from './$types';
	import { pageTitle } from '$lib/page-title';
	import {
		actionColumnClass,
		cappedColumnClass,
		cappedValueClass,
		columnFromLgClass,
		columnFromMdClass,
		columnFromSmClass,
		dragHandleClass,
		fitColumnClass,
		truncatingColumnClass
	} from '$lib/table-columns';

	type Content = components['schemas']['AdminContentResponse'];
	type ContentType = components['schemas']['ContentType'];
	type Visibility = components['schemas']['Visibility'];

	let { data }: PageProps = $props();
	let contents = $derived(data.contents);
	// 「公開できるフォルダー」が0件なら、パスを持つ種別は選ばせない (→ docs/folders.md「公開できるフォルダー」)。
	let unavailableTypes = $derived(
		data.hasSharedFolders ? [] : (['folder', 'archive'] as ContentType[])
	);
	// 登録できるのはサーバーのパソコンの前にいる管理者だけなので、編集者には頼み先を示す。
	let unavailableNote = $derived(
		isAdmin(page.data.user)
			? m.contents_create_type_needs_shared_folder_note()
			: m.contents_create_type_needs_shared_folder_note_user()
	);
	// 親カラム(パンくず)と実際の見え方の計算で、idから行を引くために使う。
	let contentById = $derived(byIdMap(contents));
	// グループの子は親の下に字下げして並べる (→ docs/ui.md「UI 全般」)。
	let rows = $derived(flattenTree(contents));

	// `content`の「親」列表示。ルート直下なら専用ラベル、それ以外は祖先パス文字列。
	function parentBreadcrumb(content: Content): string {
		if (content.parentId == null) return m.contents_table_parent_root();
		return ancestorPath(content.parentId, contentById);
	}

	// 作成ダイアログの状態。種別を選ぶ段と、必須項目を入力する段の二段階。
	let formOpen = $state(false);
	let createStep = $state<'type' | 'form'>('type');
	// 「この中に追加...」で開いたときの親グループ。
	let createParent = $state<Content | null>(null);
	const form = new ContentFormState();
	let saving = $state(false);

	let currentUserId = $derived(page.data.user?.id);
	let canSeeFullPath = $derived(canManageRoots(page.data.user, page.url.hostname));

	// 種別による制限は持たない。ログイン済みなら全部作れる (→ docs/access.md「ロールと操作」)。
	const creatableTypes: ContentType[] = ['link', 'file', 'folder', 'archive', 'group'];

	// 作成者のユーザーを削除した行は作成者が空になる (→ docs/access.md「ユーザーの削除と作成者」)。
	function creatorLabel(content: Content): string {
		return content.createdByUsername ?? m.contents_creator_deleted();
	}

	// 削除確認ダイアログの対象。
	let deletingContent = $state<Content | null>(null);
	let deleting = $state(false);
	// グループの削除でルート直下へ移り、見える範囲が広がる子。公開範囲は書き換えないので知らせる。
	let loosenedChildren = $derived(
		deletingContent?.type === 'group'
			? childrenLoosenedByDeletingGroup(deletingContent.id, contents, contentById)
			: []
	);
	// 子が多いと確定ボタンが画面の外へ押し出されるので、並べるのは先頭の数件にとどめる。
	const LOOSENED_CHILDREN_SHOWN = 5;

	let dirPickerOpen = $state(false);
	let confirmRegisterOpen = $state(false);

	let visibilityOptions = $derived(selectableVisibilities(form.type, null, currentUserId));
	let visibilityNote = $derived(contentFormVisibilityNote(form, contentById));
	let effectiveFormVisibility = $derived(contentFormEffectiveVisibility(form, contentById));

	// `folder`/`archive` の新規作成では、登録前の確認を常に出す。
	let needsRegisterConfirm = $derived(isDirectoryContentType(form.type) && form.path !== '');

	const errorDialog = new ErrorDialogState();

	// タイトルのリンクの押せる範囲を ::after で行いっぱいに広げる。つまみのある行は、つまみの右を ::after、
	// 左を ::before で覆い、つまみの下だけを空ける (→ docs/ui.md「UI 全般」)。
	// リンクは長押しで iOS のドラッグを始めないよう draggable を外し、メニューも出さない。
	const rowLinkClass =
		'truncate font-medium outline-none touch-callout-none after:absolute after:inset-y-0 after:right-0 focus-visible:after:ring-3 focus-visible:after:ring-ring/50 focus-visible:after:ring-inset';

	function openCreateDialog(parent: Content | null = null) {
		form.resetForCreate(parent === null ? 'root' : String(parent.id));
		createParent = parent;
		createStep = 'type';
		formOpen = true;
	}

	function chooseType(type: ContentType) {
		// 種別を選んだだけでは入力途中として扱わない。外側のクリックで閉じられるようにするため。
		const wasDirty = form.dirty;
		form.type = type;
		// 公開範囲はフォルダーの2段目にしか出ない。戻って別の種別を選んだときに、見えない値を残さない。
		if (type !== 'folder') form.visibility = 'public';
		if (!wasDirty) form.markPristine();
		createStep = 'form';
	}

	// 入力した値は消さない。種別を選び直しても入力し直さずに済むようにするため。
	// 送るのは選んだ種別の項目だけなので (`requestFields` / `toFormData`)、前の種別の値は送られない。
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
			if (form.type === 'file') {
				// input required属性で通常は防がれるが、念のため明示的にもチェックしておく。
				if (!form.files?.[0]) {
					errorDialog.show(GENERIC_ERROR_MESSAGE());
					return;
				}
			}
			const result =
				form.type === 'file'
					? await client.POST('/api/v1/contents/upload', {
							// openapi-fetch は FormData をそのまま送るが、生成された型はスキーマの形のままなので
							// キャストする (実行時の挙動とは無関係)。
							body: form.toFormData(
								true
							) as unknown as components['schemas']['UploadContentRequest']
						})
					: await client.POST('/api/v1/contents', {
							body: { type: form.type, ...form.createRequestFields() }
						});
			if (!result.response.ok || !result.data) {
				errorDialog.show(errorMessage(result.error));
				return;
			}
			// アーカイブは作った直後にスキャンまで済ませる。空のアーカイブが残ると、
			// 次に何を押せばよいか迷うため (→ docs/archive.md「スキャン」)。
			if (form.type === 'archive') {
				const scan = await rescanArchive(result.data.id);
				if (!scan.ok) {
					// 作成は済んでいるので、一覧に出してから、スキャンだけできなかったと伝える。
					formOpen = false;
					await invalidateAll();
					errorDialog.show(
						scan.retryable
							? m.contents_create_scan_failed({ reason: scan.message })
							: m.contents_create_scan_blocked({ reason: scan.message }),
						m.archive_scan_failed_title()
					);
					return;
				}
			}
			formOpen = false;
			// 任意の項目は、作った直後に編集ページで続けて入力する。
			await goto(resolve('/admin/contents/[id]', { id: String(result.data.id) }));
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			saving = false;
			// 成否によらず確認ダイアログは閉じる。失敗時はフォームが残るので、
			// エラー内容 (選べないパス等) を直して選び直せる。
			confirmRegisterOpen = false;
		}
	}

	// 行をグループ、または見出しの行のルートへの落とし先へドラッグして親を付け替える (→ docs/ui.md「UI 全般」)。
	let reparenting = $state(false);
	let pendingReparent = $state<{
		content: Content;
		// null はルート直下。
		target: Content | null;
		loosened: { before: Visibility; after: Visibility };
	} | null>(null);
	// 送信内容の組み立てだけに使う (画面には出さない)。
	const reparentForm = new ContentFormState();

	function isValidDropTarget(dragged: Content, target: Content | null): boolean {
		// ルート直下へは、今どこかのグループに入っているものだけ移せる。
		if (target === null) return dragged.parentId != null;
		// group 以外には移せない (サーバーの validate_parent と同じ制約)。
		// 自分自身・自分の子孫への移動は循環になるので、祖先を辿って弾く
		// (selfAndAncestors は target 自身を含むので、target === dragged も一緒に弾ける)。
		return (
			target.type === 'group' &&
			!selfAndAncestors(target.id, contentById).some((ancestor) => ancestor.id === dragged.id)
		);
	}

	const reparentDrag = new ReparentDrag<Content>({
		locked: () => reparenting,
		isDropTarget: isValidDropTarget,
		onDrop: (dragged, target) => startReparent(dragged, target)
	});
	// ルートへの落とし先は、グループに入っている行を掴んでいる間だけ出す。
	let draggingChild = $derived(
		reparentDrag.draggingId !== null && contentById.get(reparentDrag.draggingId)?.parentId != null
	);
	let draggingRow = $derived(
		reparentDrag.draggingId === null
			? undefined
			: rows.find(({ content }) => content.id === reparentDrag.draggingId)
	);

	function startReparent(content: Content, target: Content | null) {
		const parentId = target?.id ?? null;
		if (parentId === (content.parentId ?? null)) return;
		// 親の付け替えで見える範囲が広がるとき。公開範囲は書き換えないので、保存前に知らせる
		// (→ docs/access.md「親が外れるときの公開範囲」、編集ページの確認と同じ判定)。
		const loosened = loosenedByReparent(
			content,
			{ visibility: content.visibility, parentId },
			contentById
		);
		if (loosened !== null) {
			pendingReparent = { content, target, loosened };
			return;
		}
		performReparent(content, parentId);
	}

	async function performReparent(content: Content, parentId: number | null) {
		if (reparenting) return;
		reparenting = true;

		try {
			// 更新 API は全項目の置き換えなので、一覧を読み込んだ後に別の人が変えた項目を
			// 古い値で戻さないよう、送る直前に取り直した値を使う。
			const latest = await client.GET('/api/v1/admin/contents');
			const fresh = latest.data?.find((c) => c.id === content.id);
			if (!latest.response.ok || !fresh) {
				errorDialog.show(errorMessage(latest.error));
				await invalidateAll();
				return;
			}
			reparentForm.loadFrom(fresh, canSeeFullPath);
			reparentForm.parentId = parentId === null ? 'root' : String(parentId);
			const result = await reparentForm.replace(content.id);
			if (!result.response.ok || !result.data) {
				errorDialog.show(errorMessage(result.error));
				return;
			}
			await invalidateAll();
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			reparenting = false;
			pendingReparent = null;
		}
	}

	async function handleDelete() {
		if (deleting || deletingContent === null) return;
		deleting = true;
		const target = deletingContent;

		try {
			const { error, response } = await client.DELETE('/api/v1/contents/{id}', {
				params: { path: { id: target.id } }
			});
			if (!response.ok) {
				errorDialog.show(errorMessage(error));
				return;
			}
			deletingContent = null;
			// groupを削除すると配下のparentIdがサーバー側で書き換わる(ルート直下に昇格)。
			// ローカルの`contents`を手動で書き換える(reparent結果をここで再現する)よりも、
			// data.contentsを取り直す方が確実なので invalidateAll() で再取得する。
			await invalidateAll();
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			deleting = false;
		}
	}
</script>

{#snippet visibilityBadge(visibility: Visibility)}
	<span class="inline-flex items-center gap-1.5 whitespace-nowrap">
		<span
			class={[
				'size-1.5 rounded-full',
				visibility === 'public' ? 'bg-primary' : 'bg-muted-foreground'
			]}
		></span>
		{visibilityLabel(visibility)}
	</span>
{/snippet}

<svelte:head><title>{pageTitle(m.admin_contents_title())}</title></svelte:head>

<div class={adminPageClass}>
	<div class={adminPageHeaderClass}>
		<h1 class={pageHeadingTextClass}>{m.admin_contents_title()}</h1>
		<Button onclick={() => openCreateDialog()}>{m.admin_contents_add_button()}</Button>
	</div>

	{#if contents.length === 0}
		<p class="text-sm text-muted-foreground">{m.admin_contents_empty()}</p>
	{:else}
		<!-- 列は6つと操作の1つ。狭い幅では優先度の低い列から隠し、横スクロールさせない
		     (→ docs/ui.md「UI 全般」)。 -->
		<div class={adminTableBandClass}>
			<Table.Root>
				<Table.Header>
					<!-- グループに入っている行を掴んでいる間は、見出しの行をルートへ移す落とし先にする (→ docs/ui.md「UI 全般」)。
					     見出しの文字は幅を保ったまま隠し、列の幅を変えない。
					     FIX: 古い iOS の WebKit は tr を絶対配置の基準にせず表にするので (WebKit Bug 240961)、落とし先は
					     inset-0 でなく、表の上端から見出しの高さ (h-10) だけを覆う。基準が tr でも表でも同じ所に重なる。 -->
					<Table.Row class={['relative', draggingChild && '[&>th]:invisible']}>
						<Table.Head>
							{m.contents_table_title()}
							{#if draggingChild}
								<!-- ドラッグと同じく読み上げには乗せない。 -->
								<div
									{@attach reparentDrag.root()}
									data-testid="reparent-root-zone"
									aria-hidden="true"
									class={[
										'visible absolute inset-x-0 top-0 flex h-10 items-center justify-center gap-2 border-2 border-dashed px-4 text-sm',
										reparentDrag.rootTargeted
											? 'border-ring bg-accent text-foreground'
											: 'border-ring/60 bg-muted text-foreground'
									]}
								>
									<ArrowUpToLineIcon class="size-4 shrink-0" />
									{m.contents_reparent_root_zone()}
								</div>
							{/if}
						</Table.Head>
						<Table.Head class={[columnFromSmClass, fitColumnClass]}
							>{m.contents_table_type()}</Table.Head
						>
						<Table.Head class={columnFromMdClass}>{m.contents_table_parent()}</Table.Head>
						<Table.Head class={columnFromLgClass}>{m.contents_table_url_or_path()}</Table.Head>
						<Table.Head class={[columnFromSmClass, fitColumnClass]}
							>{m.contents_table_visibility()}</Table.Head
						>
						<Table.Head class={[columnFromSmClass, fitColumnClass]}
							>{m.contents_table_creator()}</Table.Head
						>
						<!-- 見出しは読み上げにだけ残す。⋮ の列は幅を取らせず、残りをタイトルに渡す。 -->
						<Table.Head class={actionColumnClass}>
							<span class="sr-only">{m.common_table_actions()}</span>
						</Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each rows as { content, depth } (content.id)}
						{@const TypeIcon = contentTypeIcon(content.type)}
						<!-- 書き換えられない行は見るだけにする (→ docs/ui.md「UI 全般」)。 -->
						{@const editable = canEditContent(page.data.user, content)}
						<!-- 行のどこを押しても編集ページへ移る。押せる範囲はタイトルのリンクの ::after で行いっぱいに広げる
					     (つまみのある行は、つまみの下だけを空ける)。
					     ⋮メニューは DOM で後ろにあるので、その上に重なって押せる (→ docs/ui.md「UI 全般」)。
					     ドロップ先の当たり判定に使うので、行の要素は全行に付ける (→ reparent-drag.svelte.ts)。
					     FIX: 古い iOS の WebKit は tr の relative を ::after の基準にせず、どの行も表全体を覆って
					     最後の行に当たる (WebKit Bug 240961)。transform なら基準にするので併せて付ける。 -->
						<Table.Row
							{@attach reparentDrag.row(content)}
							class={[
								'relative transform-gpu',
								reparentDrag.draggingId === content.id && 'opacity-60',
								reparentDrag.dropTargetId === content.id &&
									'bg-accent/50 outline-2 -outline-offset-2 outline-ring'
							]}
						>
							<Table.Cell class={truncatingColumnClass}>
								<!-- 字下げは padding で付ける。空のセルを挟むと列がずれるため。
							     スマートフォン幅では1段を狭め、タイトルに幅を回す (→ docs/ui.md「UI 全般」)。
							     種別の列を隠す幅でも種別が分かるよう、アイコンはどの幅でも出す。 -->
								<span class="flex items-center gap-2 tree-indent" style:--depth={depth}>
									<TypeIcon class="size-4 shrink-0 text-muted-foreground" />
									<span class="flex min-w-0 flex-col">
										<!-- e2e が行を特定するための目印。「親」列にもグループ名がそのまま出るため、
									     行全体の名前ではなくタイトルそのものを持つ要素で探す。 -->
										<a
											href={resolve('/admin/contents/[id]', { id: String(content.id) })}
											class={[
												rowLinkClass,
												editable
													? 'before:absolute before:inset-y-0 before:left-0 before:row-link-before-handle after:row-link-after-handle'
													: 'after:left-0'
											]}
											draggable="false"
											data-testid="content-title">{content.title}</a
										>
										<!-- スマートフォン幅では公開範囲と作成者の列を隠し、タイトルの下の2段目に出す (→ docs/ui.md「UI 全般」)。 -->
										<span
											class="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground sm:hidden"
										>
											{@render visibilityBadge(content.visibility)}
											<span aria-hidden="true">·</span>
											<span class="truncate">{creatorLabel(content)}</span>
										</span>
									</span>
									<!-- 親を付け替えるつまみ。タイトルのリンクの ::after はつまみの右から始まるが、
								     つまみの押せる範囲の広がりと重ならないよう DOM ではリンクの後ろに置いて上に重ね、
								     見た目だけ order-first で先頭に戻す。読み上げ・キーボードには乗せない (→ docs/ui.md「UI 全般」)。 -->
									{#if editable}
										<span
											{@attach reparentDrag.handle(content)}
											data-testid="reparent-handle"
											aria-hidden="true"
											class={[
												dragHandleClass,
												'z-10 order-first',
												reparenting ? 'opacity-50' : 'cursor-grab'
											]}
											oncontextmenu={(event) => event.preventDefault()}
										>
											<GripVerticalIcon class="size-4" />
										</span>
									{:else}
										<!-- つまみの幅を空けて、タイトルの頭を書き換えられる行とそろえる。 -->
										<span class="order-first size-8 shrink-0" aria-hidden="true"></span>
									{/if}
								</span>
							</Table.Cell>
							<Table.Cell class={[columnFromSmClass, fitColumnClass, 'text-muted-foreground']}>
								{contentTypeLabel(content.type)}
							</Table.Cell>
							<Table.Cell class={[columnFromMdClass, cappedColumnClass, 'text-muted-foreground']}>
								<span class={cappedValueClass}>{parentBreadcrumb(content)}</span>
							</Table.Cell>
							<Table.Cell class={[columnFromLgClass, cappedColumnClass, 'text-muted-foreground']}>
								<span class={cappedValueClass}
									>{content.url ??
										contentPathLabel(content, canSeeFullPath) ??
										content.fileName}</span
								>
							</Table.Cell>
							<Table.Cell class={[columnFromSmClass, fitColumnClass]}>
								<!-- 親グループで絞られていても、各行の設定を出す。親の公開範囲は木の上の行で分かる。
							     実際の見え方は編集ページで示す (→ docs/ui.md「UI 全般」)。 -->
								{@render visibilityBadge(content.visibility)}
							</Table.Cell>
							<Table.Cell class={[columnFromSmClass, fitColumnClass, 'text-muted-foreground']}>
								{creatorLabel(content)}
							</Table.Cell>
							<Table.Cell class={actionColumnClass}>
								<RowActionsMenu name={content.title}>
									{#if content.type === 'group'}
										<DropdownMenu.Item onSelect={() => openCreateDialog(content)}>
											{m.contents_menu_add_inside()}
										</DropdownMenu.Item>
									{/if}
									<DropdownMenu.Item>
										{#snippet child({ props })}
											<a
												href={resolve('/admin/contents/[id]', { id: String(content.id) })}
												{...props}>{editable ? m.contents_menu_edit() : m.contents_menu_open()}</a
											>
										{/snippet}
									</DropdownMenu.Item>
									{#if editable}
										<DropdownMenu.Item
											variant="destructive"
											onSelect={() => (deletingContent = content)}
										>
											{m.action_delete()}
										</DropdownMenu.Item>
									{/if}
								</RowActionsMenu>
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</div>
		<!-- 掴んだ行を指に付けて運ぶ複製 (→ docs/ui.md「UI 全般」)。当たり判定は下の行で取るので、
		     ポインターを通す。 -->
		{#if reparentDrag.ghost && draggingRow}
			{@const ghost = reparentDrag.ghost}
			{@const GhostTypeIcon = contentTypeIcon(draggingRow.content.type)}
			<!-- 指がつまみの上に留まるよう、余白・字下げ・つまみの幅を行と揃える。 -->
			<div
				aria-hidden="true"
				data-testid="reparent-ghost"
				class="pointer-events-none fixed top-0 left-0 z-30 flex items-center rounded-md border bg-background px-4 opacity-70 shadow-lg md:px-6"
				style:width="{ghost.width}px"
				style:height="{ghost.height}px"
				style:transform="translate({ghost.x}px, {ghost.y}px)"
			>
				<span class="flex min-w-0 items-center gap-2 tree-indent" style:--depth={draggingRow.depth}>
					<span class="flex size-8 shrink-0 items-center justify-center text-muted-foreground">
						<GripVerticalIcon class="size-4" />
					</span>
					<GhostTypeIcon class="size-4 shrink-0 text-muted-foreground" />
					<span class="truncate font-medium">{draggingRow.content.title}</span>
				</span>
			</div>
		{/if}
	{/if}
</div>

<Dialog.Root bind:open={formOpen}>
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
			<ContentTypeChooser
				types={creatableTypes}
				label={m.contents_create_type_label()}
				disabledTypes={unavailableTypes}
				disabledReason={m.contents_create_type_needs_shared_folder()}
				disabledNote={unavailableNote}
				onselect={chooseType}
			/>
		{:else}
			<form onsubmit={handleSubmit} class="flex flex-col gap-6">
				<ContentForm
					{form}
					variant="create"
					{visibilityOptions}
					{visibilityNote}
					onchoosepath={() => (dirPickerOpen = true)}
				/>
				<Dialog.Footer>
					<Button type="button" variant="outline" onclick={backToTypes}
						>{m.contents_create_back_button()}</Button
					>
					<!-- パスはテキスト入力ではないため、required では未選択を防げない。 -->
					<LoadingButton
						type="submit"
						loading={saving}
						disabled={isDirectoryContentType(form.type) && form.path === ''}
						>{m.action_add()}</LoadingButton
					>
				</Dialog.Footer>
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>

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
	{saving}
	onconfirm={saveContent}
/>

<ConfirmDialog
	bind:open={
		() => deletingContent !== null,
		(open) => {
			if (!open) deletingContent = null;
		}
	}
	title={m.contents_delete_confirm_title()}
	busy={deleting}
	onconfirm={handleDelete}
>
	{m.common_delete_confirm_description()}
	{#if deletingContent?.type === 'group'}
		<br />
		{m.contents_delete_confirm_group_note()}
	{/if}
	{#if loosenedChildren.length > 0}
		<br />
		{m.contents_delete_confirm_loosened_note({ count: formatNumber(loosenedChildren.length) })}
		<ul class="mt-1 list-disc pl-5 text-left">
			{#each loosenedChildren.slice(0, LOOSENED_CHILDREN_SHOWN) as { content, before } (content.id)}
				<li>
					{m.contents_delete_confirm_loosened_item({
						title: content.title,
						before: visibilityLabel(before),
						after: visibilityLabel(content.visibility)
					})}
				</li>
			{/each}
			{#if loosenedChildren.length > LOOSENED_CHILDREN_SHOWN}
				<li>
					{m.contents_delete_confirm_loosened_more({
						count: formatNumber(loosenedChildren.length - LOOSENED_CHILDREN_SHOWN)
					})}
				</li>
			{/if}
		</ul>
	{/if}
</ConfirmDialog>

<AlertDialog.Root
	bind:open={
		() => pendingReparent !== null,
		(open) => {
			if (!open) pendingReparent = null;
		}
	}
>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{m.contents_edit_loosened_title()}</AlertDialog.Title>
			<AlertDialog.Description>
				{#if pendingReparent?.target === null}
					{m.contents_reparent_to_root_loosened_description({
						title: pendingReparent.content.title,
						before: visibilityLabel(pendingReparent.loosened.before),
						after: visibilityLabel(pendingReparent.loosened.after)
					})}
				{:else if pendingReparent}
					{m.contents_reparent_loosened_description({
						title: pendingReparent.content.title,
						group: pendingReparent.target.title,
						before: visibilityLabel(pendingReparent.loosened.before),
						after: visibilityLabel(pendingReparent.loosened.after)
					})}
				{/if}
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel disabled={reparenting}>{m.action_cancel()}</AlertDialog.Cancel>
			<AlertDialog.Action
				class="relative"
				disabled={reparenting}
				onclick={() =>
					pendingReparent &&
					performReparent(pendingReparent.content, pendingReparent.target?.id ?? null)}
			>
				<LoadingLabel loading={reparenting}>{m.action_save()}</LoadingLabel>
			</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
