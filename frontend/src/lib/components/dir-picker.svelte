<script lang="ts">
	import { pickerCrumbButtonClass, pickerNavClass, pickerRowClass } from '$lib/dir-picker';
	import { client } from '$lib/api/client';
	import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
	import * as m from '$lib/paraglide/messages.js';
	import type { components } from '$lib/api/schema';
	import { LatestRequest } from '$lib/latest-request';
	import { isSamePath, pathTrail } from '$lib/path-trail';
	import { joinLocationLabels } from '$lib/root-location';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Spinner } from '$lib/components/ui/spinner';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import FileIcon from '@lucide/svelte/icons/file';
	import FolderIcon from '@lucide/svelte/icons/folder';

	type DirsResponse = components['schemas']['DirsResponse'];
	type DirEntryItem = components['schemas']['DirEntryItem'];

	type Props = {
		open: boolean;
		/** ダイアログを開いたときの初期位置。空ならルートの一覧から始める。 */
		initialPath?: string;
		/** 選べるもの。作るときはどちらも選べる (`any`)。作った後は種類が変わらないので、同じものだけを選ばせる。 */
		mode?: 'dir' | 'file' | 'any';
		/**
		 * 「このフォルダーを使用」などで選ばれた絶対パスと、画面に出す場所、ファイルか。
		 * 場所は、共有の中なら「名前 / その先」。
		 */
		onselect: (path: string, label: string, isFile: boolean) => void;
	};

	let { open = $bindable(), initialPath = '', mode = 'dir', onselect }: Props = $props();

	/** 押して選んでいるファイル。ほかの階層へ移ると外れる。 */
	let selectedFile = $state<DirEntryItem | null>(null);

	/** 一覧の入れ物。開いたときの初期フォーカス先 (下の `onOpenAutoFocus`)。 */
	let listElement = $state<HTMLDivElement | null>(null);

	let listing = $state<DirsResponse | null>(null);
	let loading = $state(false);
	let loadError = $state('');

	let crumbs = $derived(listing === null ? [] : pathTrail(listing.path, listing.root));
	// 共有の中では、先頭 (共有そのもの) にフルパスではなく名前を出す
	// (→ docs/folders.md「共有」)。起点が見つからないと `pathTrail` は切らずに全部返すので、
	// そのときは置き換えない (先頭だけ名前にすると、途中の階層が名前の後ろに続いて見える)。
	let rootName = $derived(
		listing?.rootName != null &&
			listing.root != null &&
			crumbs[0] !== undefined &&
			isSamePath(crumbs[0].path, listing.root)
			? listing.rootName
			: null
	);
	let trail = $derived(
		rootName === null ? crumbs : [{ ...crumbs[0], label: rootName }, ...crumbs.slice(1)]
	);
	let selectedLabel = $derived.by(() => {
		if (listing === null || listing.path === '') return '';
		if (rootName === null) return selectedFile?.path ?? (mode === 'file' ? '' : listing.path);
		const labels = trail.map((crumb) => crumb.label);
		if (selectedFile !== null) return joinLocationLabels([...labels, selectedFile.name]);
		return mode === 'file' ? '' : joinLocationLabels(labels);
	});
	// ファイルだけを選ぶ窓では、ファイルを押すまで確定できない。
	let canConfirm = $derived(
		listing !== null && listing.selectable && (mode !== 'file' || selectedFile !== null)
	);

	// 階層を続けて辿ると前の応答が後から届くことがあり、そのまま書き込むと表示が
	// 古い階層に戻る。
	const requests = new LatestRequest();

	async function load(path: string) {
		const isCurrent = requests.begin();
		loading = true;
		loadError = '';
		selectedFile = null;
		try {
			const { data, error, response } = await client.GET('/api/v1/fs/dirs', {
				params: { query: { path, files: mode !== 'dir' } }
			});
			if (!isCurrent()) return;
			if (!response.ok || !data) {
				// 開いた位置が辿れる範囲の外だと 422 になる。既存コンテンツのパスを初期位置に
				// 渡しているので、「共有」を消した後などに起こる
				// (→ docs/folders.md「共有」)。行き止まりにせず、上位の一覧から選び直させる。
				if (response.status === 422 && path !== '') {
					await load('');
					return;
				}
				loadError = errorMessage(error);
				return;
			}
			listing = data;
		} catch {
			if (isCurrent()) loadError = GENERIC_ERROR_MESSAGE();
		} finally {
			if (isCurrent()) loading = false;
		}
	}

	// 開くたびに初期位置から読み直す。閉じている間はリクエストを投げない。
	// 前回の表示を消してから読むのは、開いた瞬間に古い階層が出るのを防ぐため。
	$effect(() => {
		if (!open) {
			requests.invalidate();
			listing = null;
			loadError = '';
			loading = false;
			return;
		}
		listing = null;
		load(initialPath);
	});

	function confirmSelection() {
		if (listing === null || !canConfirm) return;
		onselect(selectedFile?.path ?? listing.path, selectedLabel, selectedFile !== null);
		open = false;
	}

	/** ディレクトリの行 (親へ戻る・子へ入る)。 */
</script>

{#snippet entryRow(entry: DirEntryItem)}
	<!-- 選べないディレクトリも辿れる (→ docs/folders.md「共有」)。
	     選択の可否だけを落とし、中を見ることは妨げない。 -->
	<button type="button" class={pickerRowClass} onclick={() => load(entry.path)}>
		<FolderIcon
			class="size-5 shrink-0 {entry.selectable
				? 'text-sub-foreground'
				: 'text-muted-foreground/50'}"
		/>
		<span class="flex-1 truncate text-sm {entry.selectable ? '' : 'text-muted-foreground'}"
			>{entry.name}</span
		>
		{#if !entry.selectable}
			<span class="shrink-0 text-xs text-muted-foreground">{m.dir_picker_not_selectable()}</span>
		{/if}
		<ChevronRightIcon class="size-4 shrink-0 text-muted-foreground" />
	</button>
{/snippet}

{#snippet fileRow(file: DirEntryItem)}
	<!-- 押して選ぶ。もう一度押すと外れる (フォルダーのほうを選び直せるように)。 -->
	<button
		type="button"
		class={[
			pickerRowClass,
			selectedFile?.path === file.path && 'bg-primary/10 hover:bg-primary/10'
		]}
		aria-pressed={selectedFile?.path === file.path}
		onclick={() => (selectedFile = selectedFile?.path === file.path ? null : file)}
	>
		<FileIcon class="size-5 shrink-0 text-sub-foreground" />
		<span class="flex-1 truncate text-sm">{file.name}</span>
	</button>
{/snippet}

<Dialog.Root bind:open>
	<!-- 既定では最初のフォーカス可能な要素 (ヘッダーのヘルプへのリンク) に当たってしまう。
	     開いた直後は一覧に合わせ、そのままキーで送れるようにする。 -->
	<Dialog.Content
		class="flex max-h-4/5 flex-col gap-0 p-0 sm:max-w-lg"
		onOpenAutoFocus={(event) => {
			event.preventDefault();
			listElement?.focus();
		}}
	>
		<Dialog.Header class="border-b p-4 text-left">
			<Dialog.Title>
				{mode === 'any'
					? m.dir_picker_title_any()
					: mode === 'file'
						? m.dir_picker_title_file()
						: m.dir_picker_title()}
			</Dialog.Title>
		</Dialog.Header>

		{#if listing !== null && trail.length > 0}
			<!-- パンくずは実際の高さを 44px にする (→ docs/ui.md「UI 全般」)。 -->
			<nav aria-label={m.dir_picker_breadcrumb_label()} class={pickerNavClass}>
				{#each trail as crumb, index (crumb.path)}
					{#if index > 0}
						<span aria-hidden="true" class="text-muted-foreground">›</span>
					{/if}
					{#if index === trail.length - 1}
						<span class="text-xs text-muted-foreground">{crumb.label}</span>
					{:else}
						<button type="button" class={pickerCrumbButtonClass} onclick={() => load(crumb.path)}
							>{crumb.label}</button
						>
					{/if}
				{/each}
			</nav>
		{/if}

		<div bind:this={listElement} tabindex="-1" class="min-h-0 flex-1 overflow-y-auto">
			{#if loadError !== ''}
				<p class="px-4 py-4 text-sm text-destructive">{loadError}</p>
			{:else if loading && listing === null}
				<div class="px-4 py-4">
					<Spinner aria-label={m.dir_picker_loading()} />
				</div>
			{:else if listing !== null}
				{#if listing.parent !== null && listing.parent !== undefined}
					{@const parent = listing.parent}
					<button type="button" class={pickerRowClass} onclick={() => load(parent)}>
						<ChevronLeftIcon class="size-5 shrink-0 text-muted-foreground" />
						<span class="flex-1 text-sm text-muted-foreground">{m.dir_picker_up()}</span>
					</button>
				{/if}
				{#each listing.entries as entry (entry.path)}
					{@render entryRow(entry)}
				{/each}
				{#each listing.files as file (file.path)}
					{@render fileRow(file)}
				{/each}
				{#if listing.entries.length === 0 && listing.files.length === 0}
					<p class="px-4 py-4 text-sm text-muted-foreground">
						{mode === 'dir' ? m.dir_picker_empty() : m.dir_picker_empty_any()}
					</p>
				{/if}
			{/if}
		</div>

		<div class="flex items-start gap-3 border-t bg-muted/40 px-4 pt-4 pb-3">
			<span class="shrink-0 pt-0.5 text-xs text-muted-foreground"
				>{m.dir_picker_selected_label()}</span
			>
			<!-- 省略すると末尾 (選んだフォルダー名) が隠れる。折り返して全部見せる。 -->
			<span class="min-w-0 flex-1 text-sm wrap-anywhere">
				{#if selectedLabel !== ''}
					{selectedLabel}
				{:else}
					<span class="text-muted-foreground">
						{mode === 'any'
							? m.dir_picker_selected_none_any()
							: mode === 'file'
								? m.dir_picker_selected_none_file()
								: m.dir_picker_selected_none()}
					</span>
				{/if}
			</span>
		</div>
		{#if listing !== null && listing.path !== '' && !listing.selectable}
			<p class="bg-muted/40 px-4 pb-3 text-xs text-muted-foreground">
				{m.dir_picker_current_not_selectable()}
			</p>
		{/if}
		<!-- 中身に余白 (p-0) を持たせていないので、Dialog.Footer の負の余白を打ち消し、ほかのダイアログと同じく端まで届かせる。 -->
		<Dialog.Footer class="mx-0 mb-0 border-t-0 bg-muted/40 pt-0">
			<Button type="button" variant="outline" onclick={() => (open = false)}
				>{m.action_cancel()}</Button
			>
			<Button type="button" disabled={!canConfirm} onclick={confirmSelection}>
				{selectedFile !== null || mode === 'file'
					? m.dir_picker_confirm_file()
					: m.dir_picker_confirm()}
			</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
