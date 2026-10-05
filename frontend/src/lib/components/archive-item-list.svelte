<script lang="ts">
	import { adminTableBandClass } from '$lib/page-layout';
	import { client } from '$lib/api/client';
	import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
	import { archiveItemManageDownloadHref } from '$lib/api/urls';
	import { rescanArchive } from '$lib/archive-scan';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { LatestRequest } from '$lib/latest-request';
	import * as m from '$lib/paraglide/messages.js';
	import type { components } from '$lib/api/schema';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import { Switch } from '$lib/components/ui/switch';
	import * as Table from '$lib/components/ui/table';
	import { Spinner } from '$lib/components/ui/spinner';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import PathTail from '$lib/components/path-tail.svelte';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import {
		columnFromLgClass,
		columnFromMdClass,
		fitColumnClass,
		truncatingColumnClass
	} from '$lib/table-columns';
	import { formatByteSize, formatDate, formatNumber } from '$lib/format';

	type ArchiveItem = components['schemas']['AdminArchiveItemResponse'];

	type Props = {
		contentId: number;
		// false なら見るだけにする。再スキャン・選択・公開の切り替えを出さない (→ docs/ui.md「UI 全般」)。
		editable: boolean;
	};

	let { contentId, editable }: Props = $props();

	let items = $state<ArchiveItem[]>([]);
	let loading = $state(true);
	let loadError = $state('');

	const errorDialog = new ErrorDialogState();

	// マウント時の一覧取得。`contentId` が変わっても追従する
	// (再スキャン後の読み直しはrescan()側から明示的にloadItems()を呼ぶ)。
	$effect(() => {
		loadItems(contentId);
	});

	// 別のアーカイブへ移ったり再スキャン後に読み直したりすると、前の応答が後から届くことがある。
	const requests = new LatestRequest();

	async function loadItems(id: number) {
		const isCurrent = requests.begin();
		loading = true;
		loadError = '';
		try {
			const {
				data: fetched,
				error,
				response
			} = await client.GET('/api/v1/contents/{id}/items', {
				params: { path: { id } }
			});
			if (!isCurrent()) return;
			if (!response.ok || !fetched) {
				loadError = errorMessage(error);
				return;
			}
			items = fetched;
			selectedIds = [];
		} catch {
			if (isCurrent()) loadError = GENERIC_ERROR_MESSAGE();
		} finally {
			if (isCurrent()) loading = false;
		}
	}

	let publishedCount = $derived(items.filter((item) => item.published).length);
	let showsFilter = $derived(!loading && loadError === '' && items.length > 0);
	let summary = $derived(
		m.archive_items_summary({
			count: formatNumber(items.length),
			published: formatNumber(publishedCount)
		})
	);

	// 再スキャン。成功メッセージはボタンの近くに残し続け(次の再スキャンまで消さない)、
	// 続けて一覧を読み直す(増減があるため)。
	let rescanning = $state(false);
	let rescanMessage = $state<string | null>(null);

	async function rescan() {
		if (rescanning || publishBusy) return;
		rescanning = true;
		// 失敗したときに、前回の成功の件数を今回の結果と読み違えさせない。
		rescanMessage = null;
		try {
			const scan = await rescanArchive(contentId);
			if (!scan.ok) {
				errorDialog.show(scan.message);
				return;
			}
			rescanMessage = m.archive_items_rescan_result({
				added: formatNumber(scan.result.added),
				removed: formatNumber(scan.result.removed)
			});
			await loadItems(contentId);
		} finally {
			rescanning = false;
		}
	}

	// 表示タイトルかパスに含む文字列での絞り込み (→ docs/archive.md「アイテムの公開」)。
	let filterText = $state('');

	function matchesFilter(item: ArchiveItem, text: string): boolean {
		const needle = text.trim().toLowerCase();
		return (
			needle === '' ||
			item.title.toLowerCase().includes(needle) ||
			item.relPath.toLowerCase().includes(needle)
		);
	}

	let visibleItems = $derived(items.filter((item) => matchesFilter(item, filterText)));
	let filtering = $derived(filterText.trim() !== '');

	function setFilterText(text: string) {
		filterText = text;
		selectedIds = selectedIds.filter((id) =>
			items.some((item) => item.id === id && matchesFilter(item, text))
		);
	}

	// 一覧の選択・公開切り替え。
	let selectedIds = $state<number[]>([]);
	// 読み込み中・失敗中は出さない。別のアーカイブへ移った直後、読み直しが終わるまでの
	// 一瞬、前のアーカイブの selectedIds のまま一括操作を押せてしまうのを避けるため。
	let showsBulkOps = $derived(!loading && loadError === '' && selectedIds.length > 0);

	function toggleSelectAll(checked: boolean) {
		selectedIds = checked ? visibleItems.map((item) => item.id) : [];
	}

	function toggleSelect(itemId: number, checked: boolean) {
		selectedIds = checked ? [...selectedIds, itemId] : selectedIds.filter((id) => id !== itemId);
	}

	// 単体切り替え中のitem id。
	let savingIds = $state<number[]>([]);
	// 一括で切り替え中の公開状態。スピナーは押した方のボタンにだけ出す。
	let bulkTarget = $state<boolean | null>(null);
	let bulkSaving = $derived(bulkTarget !== null);

	// 単体・一括のいずれかの公開切替が進行中か。
	let publishBusy = $derived(savingIds.length > 0 || bulkSaving);
	// 上記に再スキャンの進行中も含めた、全体の「触ってはいけない」状態。
	let anyBusy = $derived(publishBusy || rescanning);

	async function setPublished(item: ArchiveItem, published: boolean) {
		if (anyBusy) return;
		savingIds = [...savingIds, item.id];
		try {
			const {
				data: updated,
				error,
				response
			} = await client.PUT('/api/v1/contents/{id}/items/{item_id}', {
				params: { path: { id: contentId, item_id: item.id } },
				body: { published }
			});
			if (!response.ok || !updated) {
				errorDialog.show(errorMessage(error));
				return;
			}
			items = items.map((i) => (i.id === updated.id ? updated : i));
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			savingIds = savingIds.filter((id) => id !== item.id);
		}
	}

	async function setBulkPublished(published: boolean) {
		if (anyBusy || selectedIds.length === 0) return;
		bulkTarget = published;
		// 送信対象をスナップショット。選択チェックボックスはbulkSaving中disabledだが、
		// 念のためAPI送信とローカル反映で同じ対象を使うことを保証する。
		const itemIds = [...selectedIds];
		try {
			const { error, response } = await client.PUT('/api/v1/contents/{id}/items', {
				params: { path: { id: contentId } },
				body: { itemIds, published }
			});
			if (!response.ok) {
				errorDialog.show(errorMessage(error));
				return;
			}
			const publishedIds = new Set(itemIds);
			items = items.map((item) => (publishedIds.has(item.id) ? { ...item, published } : item));
			selectedIds = [];
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			bulkTarget = null;
		}
	}
</script>

<div class="flex flex-col gap-4">
	<!-- 上部 (再スキャン・件数・絞り込み) と一括操作 (選択件数・一括公開/非公開) を
	     lg で左右に並べる。一括操作には lg:ml-auto で右へ寄せる (→ 下記)。
	     絞り込みの Field は w-full を持ち、入れ子の flex コンテナに包むと
	     flex-basis の解決が崩れて極端に潰れるため、直下の兄弟のまま並べる。
	     余白は選択の有無によらず常に取る (選択のたびに高さが変わって上部がガタつかないため)。
	     選択中は行全体を画面上端に固定する (件数の多いアーカイブで全選択しても操作を見失わないため)。 -->
	<div
		class={[
			'-mx-4 flex flex-col gap-4 px-4 py-2 md:-mx-6 md:px-6 lg:flex-row lg:items-end',
			showsBulkOps && 'sticky top-0 z-10 border-b bg-background'
		]}
	>
		<!-- 狭い幅から順に「再スキャン / 件数 / 絞り込み」を縦に積み、幅が広がるにつれて段を減らす。
		     lg で1段にするときは、一覧を見る操作の絞り込みを先 (左) に置く。 -->
		<div class="flex flex-col items-start gap-1 md:flex-row md:items-center md:gap-3 lg:order-2">
			<!-- 画面の主な操作なので見た目ごと 44px (既定の大きさ) にする。一括公開・非公開は
			     選んだ行に効く表の操作なので sm のまま (→ docs/ui.md「UI 全般」)。 -->
			{#if editable}
				<LoadingButton
					variant="outline"
					loading={rescanning}
					disabled={publishBusy}
					onclick={rescan}
				>
					<RefreshCwIcon class="size-4" />
					{m.archive_items_rescan_button()}
				</LoadingButton>
			{/if}
			<!-- 読み込み中と失敗中は件数を出さない。0 件や前のアーカイブの件数を、今の中身と取り違えさせない。 -->
			{#if !loading && loadError === ''}
				<p class="text-sm text-muted-foreground">
					{#if rescanMessage}
						{m.archive_items_summary_with_rescan_result({ summary, result: rescanMessage })}
					{:else}
						{summary}
					{/if}
				</p>
			{/if}
		</div>
		{#if showsFilter}
			<Field.Field class="max-w-sm lg:order-1">
				<Field.FieldLabel for="archive-items-filter"
					>{m.archive_items_filter_label()}</Field.FieldLabel
				>
				<Input
					id="archive-items-filter"
					type="search"
					aria-describedby="archive-items-filter-status"
					bind:value={() => filterText, setFilterText}
				/>
			</Field.Field>
		{/if}
		<!-- 一括操作は右側に並べる。 -->
		{#if showsBulkOps}
			<div class="flex flex-wrap items-center gap-2 lg:order-3 lg:ml-auto">
				<p class="text-sm text-muted-foreground">
					{m.archive_items_selected_count({ count: formatNumber(selectedIds.length) })}
				</p>
				<LoadingButton
					variant="outline"
					size="sm"
					loading={bulkTarget === true}
					disabled={anyBusy}
					onclick={() => setBulkPublished(true)}
				>
					{m.archive_items_bulk_publish()}
				</LoadingButton>
				<LoadingButton
					variant="outline"
					size="sm"
					loading={bulkTarget === false}
					disabled={anyBusy}
					onclick={() => setBulkPublished(false)}
				>
					{m.archive_items_bulk_unpublish()}
				</LoadingButton>
			</div>
		{/if}
	</div>

	{#if loading}
		<Spinner aria-label={m.common_loading()} />
	{:else if loadError !== ''}
		<p class="text-sm text-destructive">{loadError}</p>
	{:else if items.length === 0}
		<p class="text-sm text-muted-foreground">{m.archive_items_empty()}</p>
	{:else}
		<!-- 入力欄の下端を再スキャンのボタンと揃えるため、件数は欄の外 (段の下) に置く。
		     打つたびに変わる件数を読み上げさせるため、要素は常に置いて中身だけ替える。
		     空のときも隠さず (隠すと最初の読み上げを落とす)、上の余白を打ち消して高さ0で置く。 -->
		<p
			id="archive-items-filter-status"
			role="status"
			class="-mt-2 text-sm text-muted-foreground empty:-mt-4"
		>
			{#if visibleItems.length === 0}
				{m.archive_items_filter_empty()}
			{:else if filtering}
				{m.archive_items_filter_count({ count: formatNumber(visibleItems.length) })}
			{/if}
		</p>
		{#if visibleItems.length > 0}
			<div class={adminTableBandClass}>
				<Table.Root>
					<Table.Header>
						<Table.Row>
							<!-- チェックボックスの押せる範囲 (44px) が1行目と重ならないよう、見出し行も 44px にする。
					     狭い幅で列が縮むと隣のタイトルのリンクと重なるので、列幅も 40px を下回らせない。 -->
							{#if editable}
								<Table.Head class="h-11 w-10 min-w-10">
									<Checkbox
										bind:checked={
											() => selectedIds.length > 0 && selectedIds.length === visibleItems.length,
											toggleSelectAll
										}
										disabled={anyBusy}
										aria-label={m.archive_items_select_all_label()}
									/>
								</Table.Head>
							{/if}
							<Table.Head>{m.archive_items_column_title()}</Table.Head>
							<!-- 狭い幅ではパスを隠し、横スクロールさせない (→ docs/ui.md「UI 全般」)。 -->
							<Table.Head class={columnFromMdClass}>{m.archive_items_column_path()}</Table.Head>
							<Table.Head class={[columnFromLgClass, fitColumnClass, 'text-right']}>
								{m.archive_items_column_size()}
							</Table.Head>
							<Table.Head class={[columnFromLgClass, fitColumnClass]}
								>{m.archive_items_column_modified()}</Table.Head
							>
							<Table.Head class={[fitColumnClass, 'text-right']}>
								{m.archive_items_column_published()}
							</Table.Head>
						</Table.Row>
					</Table.Header>
					<Table.Body>
						{#each visibleItems as item (item.id)}
							<Table.Row>
								{#if editable}
									<Table.Cell class="min-w-10">
										<Checkbox
											bind:checked={
												() => selectedIds.includes(item.id),
												(checked) => toggleSelect(item.id, checked)
											}
											disabled={anyBusy}
											aria-label={m.archive_items_select_row_label({ path: item.relPath })}
										/>
									</Table.Cell>
								{/if}
								<Table.Cell class={truncatingColumnClass}>
									<!-- 辞書を決めるときに公開せずに中身を確かめられるよう、未公開も管理用の配信で開く
								     (→ docs/archive.md「アイテムの配信」)。 -->
									<a
										href={archiveItemManageDownloadHref(contentId, item.id)}
										class="relative inline-block max-w-full truncate align-bottom after:absolute after:inset-x-0 after:-inset-y-3 hover:underline"
										target="_blank"
										rel="external noopener noreferrer"
									>
										{item.title}
									</a>
									<!-- パスの列を隠す幅では、同じタイトルの行を見分けられるよう2段目に出す (→ docs/ui.md「UI 全般」)。 -->
									<PathTail path={item.relPath} class="text-xs text-muted-foreground md:hidden" />
								</Table.Cell>
								<!-- タイトルと余りの幅を分け合う。タイトルは短い語の繰り返しで、見分けはパスに頼るため (→ docs/ui.md「UI 全般」)。 -->
								<Table.Cell
									class={[columnFromMdClass, truncatingColumnClass, 'text-muted-foreground']}
								>
									<PathTail path={item.relPath} />
								</Table.Cell>
								<!-- ファイルが消えている・読めないときは空欄 (→ docs/archive.md「エンドポイント一覧」)。 -->
								<Table.Cell
									class={[columnFromLgClass, fitColumnClass, 'text-right text-muted-foreground']}
								>
									{formatByteSize(item.size)}
								</Table.Cell>
								<Table.Cell class={[columnFromLgClass, fitColumnClass, 'text-muted-foreground']}>
									{formatDate(item.modifiedAt)}
								</Table.Cell>
								<Table.Cell class={[fitColumnClass, 'text-right']}>
									<Switch
										bind:checked={() => item.published, (v) => setPublished(item, v)}
										disabled={anyBusy || !editable}
										aria-label={m.archive_items_publish_row_label({ path: item.relPath })}
									/>
								</Table.Cell>
							</Table.Row>
						{/each}
					</Table.Body>
				</Table.Root>
			</div>
		{/if}
	{/if}
</div>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
