<script lang="ts">
	/**
	 * コンテンツの作成・編集フォームの中身。
	 *
	 * 作成 (ダイアログ) と編集 (ページ) で同じ項目を扱うため、ここに集める。
	 * 値は `ContentFormState` が持ち、この部品は見た目と入力だけを受け持つ。
	 * 送信ボタンとダイアログの枠は置かない。呼び出し側が自分の形で用意する。
	 */
	import { visibilityLabel } from '$lib/content-labels';
	import type { ContentFormPart, ContentFormState } from '$lib/content-form.svelte';
	import { isPathContentType } from '$lib/content-types';
	import { formatByteSize } from '$lib/format';
	import * as m from '$lib/paraglide/messages.js';
	import type { components } from '$lib/api/schema';
	import { Button } from '$lib/components/ui/button';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import AutoScheduleField from '$lib/components/auto-schedule-field.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import * as Select from '$lib/components/ui/select';
	import { Switch } from '$lib/components/ui/switch';
	import PatternField from '$lib/components/pattern-field.svelte';
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
		/**
		 * 出す区画。編集ページは、どの種類にもある項目と種類ごとの項目を別の区画に分ける
		 * (→ docs/ui.md「編集ページの区画」)。作成のダイアログは全部 (`all`) を1つに出す。
		 */
		part?: 'all' | ContentFormPart;
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
	};

	let {
		form,
		variant,
		part = 'all',
		parentCandidates = [],
		visibilityOptions = [],
		visibilityError = null,
		visibilityNote = null,
		onchoosepath
	}: Props = $props();

	let editing = $derived(variant === 'edit');
	let showsCommon = $derived(part !== 'specific');
	let showsSpecific = $derived(part !== 'common');
	let showsVisibility = $derived(showsCommon && (editing || isPathContentType(form.type)));
	// 作成では、group 以外はタイトルを自動で付ける。編集ページで直す。
	let showsTitle = $derived(showsCommon && (editing || form.type === 'group'));
	let showsEditOnlyCommon = $derived(showsCommon && editing);

	function parentSelectLabel(value: string): string {
		if (value === 'root') return m.contents_form_parent_none();
		return parentCandidates.find((candidate) => String(candidate.id) === value)?.label ?? '';
	}
</script>

<Field.FieldGroup>
	{#if showsSpecific && form.type === 'link'}
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
	{:else if showsSpecific && isPathContentType(form.type)}
		<Field.Field>
			<!-- 編集では選び直せても空にはできないので、ファイルの欄と同じく作成のときだけ印を付ける。 -->
			<!-- 入力欄ではないので label にしない。 -->
			<Field.FieldTitle required={!editing}>{m.contents_form_path_label()}</Field.FieldTitle>
			<!-- 絶対パスはテキスト入力させず、サーバー上の階層を辿って選ばせる
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
	{:else if showsSpecific && form.type === 'file'}
		<!-- ファイルは差し替えない (ファイルは共有の中に置く)。前から登録してあるものは、今のファイルを見せるだけ。 -->
		<Field.Field>
			<Field.FieldLabel>{m.contents_form_file_label()}</Field.FieldLabel>
			{#if form.existingFileName !== null}
				<p class="text-sm text-muted-foreground">
					{form.existingFileName}
					{#if form.existingFileSize !== null}
						({formatByteSize(form.existingFileSize)})
					{/if}
				</p>
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
	{#if showsEditOnlyCommon}
		<Field.Field>
			<Field.FieldLabel for="content-parent">{m.contents_form_parent_label()}</Field.FieldLabel>
			<!-- 親に指定できるのはgroupのみ(folderはFS実体でありDB階層とは別物)。 -->
			<Select.Root type="single" bind:value={form.parentId}>
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
		<Field.Field data-invalid={visibilityError !== null}>
			<Field.FieldLabel for="content-visibility"
				>{m.contents_form_visibility_label()}</Field.FieldLabel
			>
			<Select.Root type="single" bind:value={form.visibility}>
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
	{#if showsEditOnlyCommon}
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
	<!-- フォルダーの見せ方は作成では聞かず、編集で変える (→ docs/folders.md「2つの表示」・「見せるものの絞り込み」)。 -->
	{#if showsSpecific && editing && form.type === 'folder'}
		<PatternField
			label={m.contents_form_include_patterns_label()}
			help={m.contents_form_include_patterns_help()}
			emptyText={m.contents_form_include_patterns_empty()}
			bind:value={form.includePatterns}
		/>
		<PatternField
			label={m.contents_form_exclude_patterns_label()}
			help={m.contents_form_exclude_patterns_help()}
			emptyText={m.contents_form_exclude_patterns_empty()}
			bind:value={form.excludePatterns}
		/>
		<Field.Field orientation="horizontal">
			<Field.FieldContent>
				<Field.FieldLabel for="content-list-view"
					>{m.contents_form_list_view_label()}</Field.FieldLabel
				>
				<Field.FieldDescription>{m.contents_form_list_view_description()}</Field.FieldDescription>
			</Field.FieldContent>
			<Switch id="content-list-view" bind:checked={form.listView} />
		</Field.Field>
		{#if form.listView}
			<AutoScheduleField
				id="content-auto-rescan"
				label={m.contents_form_auto_rescan_label()}
				intervalAfter={m.contents_form_auto_rescan_interval_after()}
				bind:mode={form.autoRescanMode}
				bind:hours={form.autoRescanHours}
				bind:time={form.autoRescanTime}
			/>
		{/if}
	{/if}
</Field.FieldGroup>
