<script lang="ts">
	import { goto, refreshAll } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { client } from '$lib/api/client';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { formatNumber } from '$lib/format';
	import { pageTitle } from '$lib/page-title';
	import { canAddRoots } from '$lib/root-location';
	import { pickRootFolder } from '$lib/root-picker';
	import {
		adminEmptyTextClass,
		adminPageClass,
		adminPageHeaderClass,
		adminTableBandClass,
		pageHeadingTextClass
	} from '$lib/page-layout';
	import {
		actionColumnClass,
		columnFromSmClass,
		fitColumnClass,
		rowLinkStretchClass
	} from '$lib/table-columns';
	import * as m from '$lib/paraglide/messages.js';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import RootDeleteDialog from '$lib/components/root-delete-dialog.svelte';
	import RowActionsMenu from '$lib/components/row-actions-menu.svelte';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Table from '$lib/components/ui/table';
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
	import WarningBand from '$lib/components/warning-band.svelte';
	import { shareAccessLabel } from '$lib/share-ops';
	import type { PageProps } from './$types';
	import type { components } from '$lib/api/schema';

	type Root = components['schemas']['RootResponse'];

	let { data }: PageProps = $props();

	// 追加は OS のフォルダー選択の窓で選ぶので、サーバーの PC から開いたときだけ出す (→ docs/folders.md「選び方」)。
	let canAdd = $derived(canAddRoots(page.url.hostname));

	// 窓を開いてから、選ばれたフォルダーを登録し終えるまで。
	let adding = $state(false);
	let deletingRoot = $state<Root | null>(null);
	const errorDialog = new ErrorDialogState();

	// このパソコンに OS のフォルダー選択の窓を出し、選んだらすぐ追加して、その共有の編集ページへ移る
	// (コンテンツの追加と同じく、名前や権限を続けて決められるように)。
	// 名前はサーバーがフォルダー名で付け、重なるときは連番を付ける (→ docs/folders.md「選び方」)。
	async function handleAdd() {
		if (adding) return;
		adding = true;
		try {
			const path = await pickRootFolder(errorDialog);
			// キャンセルか、失敗 (理由はダイアログに出してある)。
			if (path == null) return;
			const result = await errorDialog.attempt(() =>
				client.POST('/api/v1/admin/roots', { body: { path } })
			);
			if (result?.data) await goto(resolve('/admin/roots/[id]', { id: String(result.data.id) }));
		} finally {
			adding = false;
		}
	}
</script>

<svelte:head><title>{pageTitle(m.admin_roots_nav_title())}</title></svelte:head>

<div class={adminPageClass}>
	<!-- 追加ボタンは、ほかの管理画面 (コンテンツ・ユーザー) と同じく見出しの段の右に置く。 -->
	<div class={adminPageHeaderClass}>
		<h1 class={pageHeadingTextClass}>{m.admin_roots_nav_title()}</h1>
		{#if canAdd}
			<LoadingButton loading={adding} onclick={handleAdd}
				>{m.admin_roots_add_button()}</LoadingButton
			>
		{/if}
	</div>
	{#if !canAdd}
		<p class="mb-6 text-sm text-muted-foreground">{m.admin_roots_add_local_only()}</p>
	{/if}

	<!-- 行ごとには短く出し、何を確かめればよいかは一覧の上に1回だけ出す。 -->
	{#if data.roots.some((root) => root.status === 'unreachable')}
		<WarningBand class="mb-4 rounded-md border">
			<p class="text-sm leading-relaxed">{m.admin_roots_unreachable_warning()}</p>
		</WarningBand>
	{/if}
	<!-- 取り直すのは編集ページの「フォルダーを選び直す...」から (この PC の窓で選ぶため)。 -->
	{#if data.roots.some((root) => !root.writable)}
		<WarningBand class="mb-4 rounded-md border">
			<p class="text-sm leading-relaxed">{m.admin_roots_not_writable_warning()}</p>
		</WarningBand>
	{/if}

	{#if data.roots.length === 0}
		<p class={adminEmptyTextClass}>{m.admin_roots_empty()}</p>
	{:else}
		<div class={adminTableBandClass}>
			<Table.Root>
				<Table.Header>
					<Table.Row>
						<Table.Head>{m.admin_roots_name_label()}</Table.Head>
						<Table.Head class={[columnFromSmClass, fitColumnClass]}
							>{m.admin_roots_table_access()}</Table.Head
						>
						<Table.Head class={[columnFromSmClass, fitColumnClass]}
							>{m.admin_roots_table_exceptions()}</Table.Head
						>
						<Table.Head class={[columnFromSmClass, fitColumnClass]}
							>{m.admin_roots_table_contents()}</Table.Head
						>
						<!-- 見出しは読み上げにだけ残す (コンテンツ管理と同じ。→ docs/ui.md「UI 全般」)。 -->
						<Table.Head class={actionColumnClass}>
							<span class="sr-only">{m.common_table_actions()}</span>
						</Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each data.roots as root (root.id)}
						{@const access = shareAccessLabel(root.editorAccess)}
						{@const contentCount = m.admin_roots_table_content_count({
							count: formatNumber(root.contentCount)
						})}
						<!-- 行のどこを押しても編集ページへ移る (コンテンツ管理の表と同じ)。押せる範囲は名前のリンクの
						     ::after で行いっぱいに広げ、⋮は DOM で後ろに置いてその上に重ねる (→ docs/ui.md「UI 全般」)。
						     FIX: 古い iOS の WebKit は tr の relative を ::after の基準にしない (WebKit Bug 240961)。
						     transform なら基準にするので併せて付ける。 -->
						<Table.Row class="relative transform-gpu">
							<!-- パスと警告は読めないと困るので、ほかの表のタイトルと違って切らずに折り返す (→ docs/ui.md「UI 全般」)。 -->
							<Table.Cell class="whitespace-normal">
								<a
									href={resolve('/admin/roots/[id]', { id: String(root.id) })}
									class={[rowLinkStretchClass, 'block wrap-anywhere after:left-0']}
									data-testid="root-name">{root.name}</a
								>
								<!-- 名前と併せてフルパスも出す (→ docs/folders.md「共有」)。 -->
								<span class="mt-0.5 block text-xs wrap-anywhere text-muted-foreground"
									>{root.path}</span
								>
								<!-- スマートフォン幅では権限・例外・件数の列を隠し、名前の下に出す (→ docs/ui.md「UI 全般」)。 -->
								<span class="mt-0.5 block text-xs text-muted-foreground sm:hidden">
									{access}
									{#if root.userAccess.length > 0}
										<span aria-hidden="true">·</span>
										{m.admin_roots_exceptions({ count: root.userAccess.length })}
									{/if}
									<span aria-hidden="true">·</span>
									{contentCount}
								</span>
								<!-- 閲覧側では開いたときに初めて分かるので、管理の一覧で先に気づけるようにする (→ docs/folders.md「つながっているかの確かめ」)。 -->
								{#if root.status !== 'ready'}
									<span class="mt-1 flex items-start gap-1.5 text-xs text-warning-muted">
										<TriangleAlertIcon class="mt-px size-3.5 shrink-0 text-warning-icon" />
										{root.status === 'unreachable'
											? m.admin_roots_status_unreachable()
											: m.admin_roots_status_missing()}
									</span>
								{/if}
								{#if !root.writable}
									<span class="mt-1 flex items-start gap-1.5 text-xs text-warning-muted">
										<TriangleAlertIcon class="mt-px size-3.5 shrink-0 text-warning-icon" />
										{m.admin_roots_not_writable()}
									</span>
								{/if}
							</Table.Cell>
							<!-- 一般ユーザーがこの中でできること。例外は人数だけを出す (中身は編集ページで見る)。 -->
							<Table.Cell class={[columnFromSmClass, fitColumnClass]}>{access}</Table.Cell>
							<Table.Cell class={[columnFromSmClass, fitColumnClass, 'text-muted-foreground']}>
								{root.userAccess.length > 0
									? m.admin_roots_table_exceptions_count({ count: root.userAccess.length })
									: m.admin_roots_table_no_exceptions()}
							</Table.Cell>
							<Table.Cell class={[columnFromSmClass, fitColumnClass, 'text-muted-foreground']}>
								{contentCount}
							</Table.Cell>
							<Table.Cell class={actionColumnClass}>
								<RowActionsMenu name={root.name}>
									<!-- 名前の変更・権限・場所の選び直しは、どれも編集ページで行う。 -->
									<DropdownMenu.Item>
										{#snippet child({ props })}
											<a href={resolve('/admin/roots/[id]', { id: String(root.id) })} {...props}
												>{m.action_edit_page()}</a
											>
										{/snippet}
									</DropdownMenu.Item>
									<DropdownMenu.Item variant="destructive" onSelect={() => (deletingRoot = root)}>
										{m.action_delete()}
									</DropdownMenu.Item>
								</RowActionsMenu>
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</div>
	{/if}
</div>

<RootDeleteDialog bind:root={deletingRoot} ondeleted={() => refreshAll()} />

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
