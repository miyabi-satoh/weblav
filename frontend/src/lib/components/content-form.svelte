<script lang="ts">
	/**
	 * コンテンツの作成・編集フォームの中身。
	 *
	 * 作成 (ダイアログ) と編集 (ページ) で同じ項目を扱うため、ここに集める。
	 * 値は `ContentFormState` が持ち、この部品は見た目と入力だけを受け持つ。
	 * 送信ボタンとダイアログの枠は置かない。呼び出し側が自分の形で用意する。
	 */
	import { visibilityLabel } from '$lib/content-labels';
	import type { ContentFormState } from '$lib/content-form.svelte';
	import { isDirectoryContentType } from '$lib/content-types';
	import { formatByteSize } from '$lib/format';
	import * as m from '$lib/paraglide/messages.js';
	import type { components } from '$lib/api/schema';
	import { Button } from '$lib/components/ui/button';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import FileDropzone from '$lib/components/file-dropzone.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import * as Select from '$lib/components/ui/select';
	import { Textarea } from '$lib/components/ui/textarea';

	type Visibility = components['schemas']['Visibility'];

	type Props = {
		form: ContentFormState;
		/**
		 * `create` は作成ダイアログの2段目。種別は1段目で選び済みで、その種別の必須項目だけを出す。
		 * 公開範囲はフォルダーにだけ出す (ファイル単位で隠せず、登録した時点で公開されるため
		 * → docs/folders.md「登録前の確認」)。
		 * `edit` は編集ページ。すべての項目を出す。種別は変えられず、ファイルは差し替えの任意項目になる。
		 */
		variant: 'create' | 'edit';
		/** 親グループの候補。ラベルは祖先を辿ったパス。`edit` だけで使う。 */
		parentCandidates?: { id: number; label: string }[];
		/** 選べる公開範囲。呼び出し側が `selectableVisibilities` で算出する。 */
		visibilityOptions?: Visibility[];
		/** 公開範囲が保存できないときの理由。 */
		visibilityError?: string | null;
		/** 親グループによって実際の見え方が設定より厳しいときの説明 (→ docs/ui.md「UI 全般」)。 */
		visibilityNote?: string | null;
		/** パスを選ぶダイアログを開く。 */
		onchoosepath: () => void;
		/**
		 * 見るだけにする (→ docs/ui.md「UI 全般」)。ネイティブの入力欄は呼び出し側の `<fieldset disabled>` で
		 * 止まるが、選択 (bits-ui) とファイルのドロップは止まらないので、ここで止める。
		 */
		disabled?: boolean;
	};

	let {
		form,
		variant,
		parentCandidates = [],
		visibilityOptions = [],
		visibilityError = null,
		visibilityNote = null,
		onchoosepath,
		disabled = false
	}: Props = $props();

	let editing = $derived(variant === 'edit');
	let showsVisibility = $derived(editing || form.type === 'folder');
	// 作成では、group 以外はタイトルを自動で付ける。編集ページで直す。
	let showsTitle = $derived(editing || form.type === 'group');

	function parentSelectLabel(value: string): string {
		if (value === 'root') return m.contents_form_parent_none();
		return parentCandidates.find((candidate) => String(candidate.id) === value)?.label ?? '';
	}
</script>

<Field.FieldGroup>
	{#if form.type === 'link'}
		<Field.Field>
			<Field.FieldLabel for="content-url" required>{m.contents_form_url_label()}</Field.FieldLabel>
			<!-- type="url" だとブラウザーが `example.com` のようにプロトコルを省いた値を弾くので、text にする。
			     スキームはサーバーが補う (→ docs/ui.md「UI 全般」)。inputmode でスマートフォンの URL 用キーボードは残す。 -->
			<Input
				id="content-url"
				type="text"
				inputmode="url"
				autocomplete="url"
				bind:value={form.url}
				required
			/>
		</Field.Field>
	{:else if isDirectoryContentType(form.type)}
		<Field.Field>
			<!-- 編集では選び直せても空にはできないので、ファイルの欄と同じく作成のときだけ印を付ける。 -->
			<Field.FieldLabel required={!editing}>{m.contents_form_path_label()}</Field.FieldLabel>
			<!-- 絶対パスはテキスト入力させず、サーバー上のディレクトリを辿って選ばせる
			     (→ docs/folders.md「登録前の確認」)。 -->
			<div class="flex items-center gap-3">
				<!-- 省略すると末尾 (選んだフォルダー名) が隠れる。折り返して全部見せる。 -->
				<span class="min-w-0 flex-1 text-sm wrap-anywhere">
					{#if form.path === ''}
						<span class="text-muted-foreground">{m.contents_form_path_unset()}</span>
					{:else}
						{form.pathLabel}
					{/if}
				</span>
				<Button type="button" variant="outline" size="sm" onclick={onchoosepath}>
					{form.path === '' ? m.contents_form_path_choose() : m.contents_form_path_change()}
				</Button>
			</div>
		</Field.Field>
		{#if editing && form.type === 'archive'}
			<Field.Field>
				<Field.FieldLabel for="content-extensions"
					>{m.contents_form_extensions_label()}</Field.FieldLabel
				>
				<Input id="content-extensions" bind:value={form.extensions} />
				<Field.FieldDescription>{m.contents_form_extensions_help()}</Field.FieldDescription>
			</Field.Field>
		{/if}
	{:else if form.type === 'file'}
		<Field.Field>
			<Field.FieldLabel for="content-file" required={!editing}
				>{m.contents_form_file_label()}</Field.FieldLabel
			>
			{#if editing}
				{#if form.existingFileName !== null}
					<p class="text-sm text-muted-foreground">
						{m.contents_form_file_current_label()}: {form.existingFileName}
						{#if form.existingFileSize !== null}
							({formatByteSize(form.existingFileSize)})
						{/if}
					</p>
				{/if}
				{#if !disabled}
					{#key form.fileInputVersion}
						<FileDropzone
							id="content-file"
							bind:files={form.files}
							hint={m.contents_form_dropzone_replace_hint()}
						/>
					{/key}
				{/if}
			{:else}
				{#key form.fileInputVersion}
					<FileDropzone
						id="content-file"
						bind:files={form.files}
						required
						hint={m.contents_form_dropzone_hint()}
					/>
				{/key}
			{/if}
		</Field.Field>
	{/if}
	{#if showsTitle}
		<Field.Field>
			<Field.FieldLabel for="content-title" required
				>{m.contents_form_title_label()}</Field.FieldLabel
			>
			<Input id="content-title" bind:value={form.title} required />
			<!-- 説明の「再取得」と同じく、入力欄の下に置く。 -->
			{#if editing && form.type === 'link'}
				<div>
					<LoadingButton
						type="button"
						variant="outline"
						size="sm"
						loading={form.titleRefetchStatus === 'loading'}
						disabled={form.url.trim() === ''}
						onclick={() => form.refetchTitleFromUrl()}
					>
						{m.contents_form_link_refetch_title()}
					</LoadingButton>
				</div>
			{/if}
			{#if form.titleRefetchStatus === 'failed'}
				<Field.FieldError>{m.contents_form_link_refetch_failed()}</Field.FieldError>
			{/if}
		</Field.Field>
	{/if}
	{#if editing}
		<Field.Field>
			<Field.FieldLabel for="content-parent">{m.contents_form_parent_label()}</Field.FieldLabel>
			<!-- 親に指定できるのはgroupのみ(folderはFS実体でありDB階層とは別物)。 -->
			<Select.Root type="single" bind:value={form.parentId} {disabled}>
				<Select.Trigger id="content-parent" class="w-full">
					{parentSelectLabel(form.parentId)}
				</Select.Trigger>
				<Select.Content>
					<Select.Item value="root">{m.contents_form_parent_none()}</Select.Item>
					{#each parentCandidates as candidate (candidate.id)}
						<Select.Item value={String(candidate.id)}>{candidate.label}</Select.Item>
					{/each}
				</Select.Content>
			</Select.Root>
		</Field.Field>
	{/if}
	{#if showsVisibility}
		<Field.Field>
			<Field.FieldLabel for="content-visibility"
				>{m.contents_form_visibility_label()}</Field.FieldLabel
			>
			<Select.Root type="single" bind:value={form.visibility} {disabled}>
				<Select.Trigger
					id="content-visibility"
					class="w-full"
					aria-invalid={visibilityError !== null}
				>
					{visibilityLabel(form.visibility)}
				</Select.Trigger>
				<Select.Content>
					{#each visibilityOptions as option (option)}
						<Select.Item value={option}>{visibilityLabel(option)}</Select.Item>
					{/each}
				</Select.Content>
			</Select.Root>
			{#if visibilityError !== null}
				<Field.FieldError>{visibilityError}</Field.FieldError>
			{/if}
			{#if visibilityNote !== null}
				<Field.FieldDescription>{visibilityNote}</Field.FieldDescription>
			{/if}
		</Field.Field>
	{/if}
	{#if editing}
		<Field.Field>
			<Field.FieldLabel for="content-description"
				>{m.contents_form_description_label()}</Field.FieldLabel
			>
			<Textarea id="content-description" bind:value={form.description} />
			{#if form.type === 'link'}
				<div>
					<LoadingButton
						type="button"
						variant="outline"
						size="sm"
						loading={form.descriptionRefetchStatus === 'loading'}
						disabled={form.url.trim() === ''}
						onclick={() => form.refetchDescriptionFromUrl()}
					>
						{m.contents_form_link_refetch_description()}
					</LoadingButton>
				</div>
				{#if form.descriptionRefetchStatus === 'failed'}
					<Field.FieldError>{m.contents_form_link_refetch_failed()}</Field.FieldError>
				{/if}
			{/if}
		</Field.Field>
	{/if}
</Field.FieldGroup>
