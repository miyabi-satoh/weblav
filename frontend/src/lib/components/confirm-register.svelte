<script lang="ts">
	import { client } from '$lib/api/client';
	import { GENERIC_ERROR_MESSAGE, errorMessage, unreadableFolderMessage } from '$lib/api/errors';
	import * as m from '$lib/paraglide/messages.js';
	import type { components } from '$lib/api/schema';
	import { contentTypeLabel } from '$lib/content-labels';
	import { LatestRequest } from '$lib/latest-request';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import ScanProgress from '$lib/components/scan-progress.svelte';
	import { Spinner } from '$lib/components/ui/spinner';
	import WarningBand from '$lib/components/warning-band.svelte';
	import { formatNumber } from '$lib/format';

	type ContentType = components['schemas']['ContentType'];
	type Visibility = components['schemas']['Visibility'];

	type Props = {
		open: boolean;
		/** 登録しようとしているコンテンツの種類。件数の数え方と文言が変わる。 */
		contentType: ContentType;
		/** 件数を数える絶対パス。 */
		path: string;
		/** 画面に出す場所 (→ `$lib/root-location`)。 */
		pathLabel: string;
		/**
		 * 実際の見え方。設定ではなく、親グループの公開範囲を重ねた値を渡す
		 * (→ docs/folders.md「登録前の確認」)。
		 */
		visibility: Visibility;
		/** 一覧の表示で索引する拡張子 (カンマ区切り)。件数の数え方を絞り込む。 */
		extensions?: string;
		/** 一覧の表示を使うか。読めない場所があると索引できないことを伝える。 */
		listView?: boolean;
		/** 新しく見つかったファイルを公開するか (→ docs/folders.md「公開と非公開」)。 */
		newFilesPublic?: boolean;
		/** 登録済みのフォルダーの場所を変える保存か。変えると公開・非公開の印が消える。 */
		clearsMarks?: boolean;
		/** 編集ページの保存か。作るときの「登録」でなく「保存」の言い方にする。 */
		editing?: boolean;
		/** 保存処理中か。確認後の送信は呼び出し側が行う。 */
		saving?: boolean;
		/** 保存に続くスキャンの進み具合。押したボタンの近くに出す (→ docs/archive.md「スキャン」)。 */
		progress?: string;
		/** 進み具合の横の「中止する」で呼ぶ。 */
		oncancelscan?: () => Promise<void>;
		onconfirm: () => void;
	};

	let {
		open = $bindable(),
		contentType,
		path,
		pathLabel,
		visibility,
		extensions,
		listView = false,
		newFilesPublic = true,
		clearsMarks = false,
		editing = false,
		saving = false,
		progress,
		oncancelscan,
		onconfirm
	}: Props = $props();

	let count = $state<number | null>(null);
	let truncated = $state(false);
	/** 最初に読めなかった場所。読めない場所があると、アーカイブは再スキャンできない (→ docs/archive.md「スキャン」)。 */
	let unreadable = $state<string | null>(null);
	let loading = $state(false);
	let loadError = $state('');

	// 閉じた後や対象パスが変わった後に古い応答が届くと、別のパスの件数を出してしまう。
	const requests = new LatestRequest();

	// 開くたびに数え直す。件数は確認のためにその場で数えるだけで保持しない
	// (→ docs/folders.md「登録前の確認」)。
	$effect(() => {
		if (!open) {
			requests.invalidate();
			count = null;
			truncated = false;
			unreadable = null;
			loadError = '';
			loading = false;
			return;
		}
		countFiles(path, extensions);
	});

	async function countFiles(target: string, targetExtensions: string | undefined) {
		const isCurrent = requests.begin();
		loading = true;
		loadError = '';
		count = null;
		truncated = false;
		unreadable = null;
		try {
			const { data, error, response } = await client.GET('/api/v1/admin/fs/count', {
				params: { query: { path: target, extensions: targetExtensions } }
			});
			if (!isCurrent()) return;
			if (!response.ok || !data) {
				loadError = errorMessage(error);
				return;
			}
			count = data.count;
			truncated = data.truncated;
			unreadable = data.unreadable ?? null;
		} catch {
			if (isCurrent()) loadError = GENERIC_ERROR_MESSAGE();
		} finally {
			if (isCurrent()) loading = false;
		}
	}

	let formattedCount = $derived(formatNumber(count));
	let archiveUnscannable = $derived(listView && unreadable !== null);

	/** 件数の見出し。打ち切られた場合は下限値であることを示す。 */
	let countHeadline = $derived(
		count === null
			? ''
			: truncated
				? m.confirm_register_count_truncated()
				: m.confirm_register_count()
	);

	/**
	 * 実効的な閲覧範囲の説明 (→ docs/folders.md「登録前の確認」)。
	 *
	 * 新しく見つかったファイルを公開しないフォルダーは、登録した時点では何も見えないので、
	 * 公開範囲による文言分岐が無く、`visibility` を見ない専用の文言にする。
	 * 件数が打ち切られたときは、見出しと同じく下限であることを示す。
	 */
	let scopeMessage = $derived.by(() => {
		const params = { count: formattedCount };
		if (!newFilesPublic) {
			return truncated
				? m.confirm_register_scope_chosen_truncated(params)
				: m.confirm_register_scope_chosen(params);
		}
		if (visibility === 'public') {
			return truncated
				? m.confirm_register_scope_folder_public_truncated(params)
				: m.confirm_register_scope_folder_public(params);
		}
		if (visibility === 'authenticated') return m.confirm_register_scope_folder_authenticated();
		if (visibility === 'private') return m.confirm_register_scope_folder_private();
		return m.confirm_register_scope_folder_hidden();
	});

	let noteMessage = $derived(
		clearsMarks
			? m.confirm_register_note_path_changed()
			: newFilesPublic
				? m.confirm_register_note_folder()
				: null
	);
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="sm:max-w-lg">
		<Dialog.Header class="text-left">
			<Dialog.Title
				>{editing ? m.confirm_register_save_title() : m.confirm_register_title()}</Dialog.Title
			>
		</Dialog.Header>
		<Dialog.Body>
			<dl class="min-w-0 text-sm">
				<div class="flex min-w-0 gap-4 border-b border-divider pb-4">
					<dt class="w-16 shrink-0 text-xs text-muted-foreground">
						{m.confirm_register_type_label()}
					</dt>
					<dd class="flex-1">{contentTypeLabel(contentType)}</dd>
				</div>
				<div class="flex min-w-0 gap-4 border-b border-divider py-4">
					<dt class="w-16 shrink-0 text-xs text-muted-foreground">
						{m.confirm_register_path_label()}
					</dt>
					<!-- パスは省略せず折り返す。この画面の用は「指定を間違えていないか
				     確かめること」なので、末尾を切ると役に立たない。 -->
					<dd class="min-w-0 flex-1 text-sm wrap-anywhere">{pathLabel}</dd>
				</div>
			</dl>

			{#if loadError !== ''}
				<p class="text-sm text-destructive">{loadError}</p>
			{:else if loading}
				<Spinner aria-label={m.confirm_register_counting()} />
			{:else if count !== null}
				<p class="flex items-baseline gap-2.5">
					<span class="text-4xl leading-none tracking-tight">{formattedCount}</span>
					<span class="text-sm text-muted-foreground">{countHeadline}</span>
				</p>
				{#if unreadable !== null}
					<p class="text-sm text-destructive">{unreadableFolderMessage(unreadable)}</p>
				{/if}

				<!-- 読めない場所があるアーカイブは、直すまで1件も索引されないので「N 個が索引されます」は出さない。 -->
				{#if !archiveUnscannable || noteMessage !== null}
					<WarningBand>
						{#if !archiveUnscannable}
							<p class="text-sm leading-relaxed">{scopeMessage}</p>
						{/if}
						{#if noteMessage !== null}
							<p
								class={[
									'text-xs leading-relaxed text-warning-muted',
									!archiveUnscannable && 'mt-1.5'
								]}
							>
								{noteMessage}
							</p>
						{/if}
					</WarningBand>
				{/if}
			{/if}
			{#if progress && oncancelscan}
				<ScanProgress message={progress} oncancel={oncancelscan} />
			{:else if progress}
				<p role="status" class="text-sm text-muted-foreground">{progress}</p>
			{/if}
		</Dialog.Body>

		<Dialog.Footer>
			<Button type="button" variant="outline" disabled={saving} onclick={() => (open = false)}
				>{m.confirm_register_back()}</Button
			>
			<LoadingButton
				type="button"
				loading={saving}
				disabled={loadError !== '' || loading}
				onclick={onconfirm}>{editing ? m.action_save() : m.confirm_register_action()}</LoadingButton
			>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
