<script lang="ts">
	import { untrack } from 'svelte';
	import CheckIcon from '@lucide/svelte/icons/check';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import { toast } from 'svelte-sonner';
	import { client } from '$lib/api/client';
	import { errorCode, errorDetail, errorMessage, GENERIC_ERROR_MESSAGE } from '$lib/api/errors';
	import type { components } from '$lib/api/schema';
	import {
		buildAxesPrompt,
		parseAxesAnswer,
		type ImportAxesRequest,
		type ParseFailure
	} from '$lib/axes-ai-prompt';
	import { copyText } from '$lib/clipboard';
	import { CopiedState } from '$lib/copied-state.svelte';
	import { FormDirtyState } from '$lib/form-dirty.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import { Spinner } from '$lib/components/ui/spinner';
	import { Textarea } from '$lib/components/ui/textarea';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import WarningBand from '$lib/components/warning-band.svelte';
	import * as m from '$lib/paraglide/messages.js';

	let {
		open = $bindable(false),
		contentId,
		onimported
	}: {
		open?: boolean;
		contentId: number;
		/** 取り込んだ後に呼ぶ。軸の一覧と表示タイトルを読み直すため。 */
		onimported: () => Promise<void>;
	} = $props();

	type Preview = components['schemas']['ImportPreviewResponse'];

	let prompt = $state<string | null>(null);
	let promptFailed = $state(false);
	let answer = $state('');
	let checking = $state(false);
	let importing = $state(false);
	let checkError = $state<string | null>(null);
	/** 確かめた答えと、その予告。答えを書き換えたら取り込めないようにするため、両方を持つ。 */
	let checked = $state<{ answer: string; request: ImportAxesRequest; preview: Preview } | null>(
		null
	);
	let ready = $derived(checked !== null && checked.answer === answer);
	const copyFeedback = new CopiedState();
	// 貼った答えを、外側を押したり Esc で閉じたりして失わないようにする。
	const answerForm = new FormDirtyState(() => ({ answer }));
	let previewSection = $state<HTMLElement | null>(null);

	// 予告は長い答えの欄の下に出るので、確かめたら見える位置まで送る。
	$effect(() => {
		previewSection?.scrollIntoView({ block: 'start', behavior: 'smooth' });
	});

	$effect(() => {
		// 開いたときだけ読み直す。loadPrompt の中で読む答えの欄を依存に入れると、入力のたびに消える。
		if (open) untrack(() => void loadPrompt());
	});

	async function loadPrompt() {
		prompt = null;
		promptFailed = false;
		answer = '';
		answerForm.markPristine();
		checked = null;
		checkError = null;
		const path = { params: { path: { id: contentId } } };
		try {
			const [levels, words, items] = await Promise.all([
				client.GET('/api/v1/contents/{id}/dir-levels', path),
				client.GET('/api/v1/contents/{id}/filename-words', path),
				client.GET('/api/v1/contents/{id}/items', path)
			]);
			if (!levels.data || !words.data || !items.data) {
				promptFailed = true;
				return;
			}
			prompt = buildAxesPrompt({
				itemCount: items.data.length,
				levels: levels.data,
				words: words.data.words,
				relPaths: items.data.map((item) => item.relPath)
			});
		} catch {
			promptFailed = true;
		}
	}

	async function copyPrompt() {
		if (!prompt) return;
		try {
			await copyText(prompt);
			copyFeedback.show();
		} catch {
			toast.error(m.common_copy_failed_toast());
		}
	}

	const PARSE_FAILURE_MESSAGES: Record<ParseFailure, () => string> = {
		notJson: m.archive_axes_ai_error_not_json,
		notAxes: m.archive_axes_ai_error_not_axes
	};

	async function check() {
		const current = answer;
		checked = null;
		checkError = null;
		const parsed = parseAxesAnswer(current);
		if (!parsed.ok) {
			checkError = PARSE_FAILURE_MESSAGES[parsed.reason]();
			return;
		}
		checking = true;
		try {
			const { data, error } = await client.POST('/api/v1/contents/{id}/axes/import/preview', {
				params: { path: { id: contentId } },
				body: parsed.value
			});
			if (data) {
				checked = { answer: current, request: parsed.value, preview: data };
			} else {
				checkError = answerErrorMessage(error);
			}
		} catch {
			checkError = GENERIC_ERROR_MESSAGE();
		} finally {
			checking = false;
		}
	}

	/**
	 * どの軸の何が違うかが分かる 422 はその文言を、形が読めない答え (型の違い・知らない抽出元) は
	 * 汎用の文言でなく頼み直し方を出す。どちらも code は同じなので、内訳の有無で分ける。
	 */
	function answerErrorMessage(error: unknown): string {
		if (errorDetail(error) === undefined && errorCode(error) === 'invalid_request_body') {
			return m.archive_axes_ai_error_shape();
		}
		return errorMessage(error);
	}

	async function importAxes() {
		if (!checked || !ready) return;
		importing = true;
		try {
			const { error } = await client.PUT('/api/v1/contents/{id}/axes', {
				params: { path: { id: contentId } },
				body: checked.request
			});
			if (error) {
				checkError = answerErrorMessage(error);
				checked = null;
				return;
			}
		} catch {
			checkError = GENERIC_ERROR_MESSAGE();
			return;
		} finally {
			importing = false;
		}
		// 保存はできているので、読み直しに失敗しても取り込みは失敗と見せない。
		open = false;
		toast.success(m.archive_axes_ai_imported_toast());
		await onimported().catch(() => toast.error(GENERIC_ERROR_MESSAGE()));
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content
		class="sm:max-w-2xl"
		interactOutsideBehavior={answerForm.dirty ? 'ignore' : 'close'}
		escapeKeydownBehavior={answerForm.dirty ? 'ignore' : 'close'}
	>
		<Dialog.Header>
			<Dialog.Title>{m.archive_axes_ai_dialog_title()}</Dialog.Title>
			<Dialog.Description>{m.archive_axes_ai_dialog_description()}</Dialog.Description>
		</Dialog.Header>

		<section class="flex min-w-0 flex-col gap-2">
			<h3 class="text-sm font-semibold">{m.archive_axes_ai_step_copy()}</h3>
			<p class="text-sm text-muted-foreground">{m.archive_axes_ai_privacy_note()}</p>
			{#if prompt !== null}
				<div class="flex items-start gap-2">
					<Textarea
						readonly
						value={prompt}
						rows={6}
						class="max-h-48 min-w-0 flex-1 font-mono text-xs"
						aria-label={m.archive_axes_ai_prompt_label()}
					/>
					<Button
						variant="outline"
						size="icon-sm"
						onclick={copyPrompt}
						aria-label={m.archive_axes_ai_copy_button()}
					>
						{#if copyFeedback.copied}
							<CheckIcon />
						{:else}
							<CopyIcon />
						{/if}
					</Button>
				</div>
			{:else if promptFailed}
				<p role="alert" class="text-sm text-destructive">{m.archive_axes_ai_prompt_failed()}</p>
			{:else}
				<Spinner aria-label={m.common_loading()} />
			{/if}
		</section>

		<section class="flex min-w-0 flex-col gap-2">
			<Field.Field>
				<Field.FieldLabel for="axes-ai-answer">{m.archive_axes_ai_step_paste()}</Field.FieldLabel>
				<Textarea
					id="axes-ai-answer"
					bind:value={answer}
					rows={6}
					class="max-h-64 font-mono text-xs"
				/>
			</Field.Field>
			<LoadingButton
				variant="outline"
				class="w-fit"
				loading={checking}
				disabled={answer.trim() === '' || ready}
				onclick={check}
			>
				{m.archive_axes_ai_check_button()}
			</LoadingButton>
			{#if checkError}
				<p role="alert" class="text-sm text-destructive">{checkError}</p>
			{/if}
		</section>

		{#if checked && ready}
			{@const preview = checked.preview}
			<section bind:this={previewSection} class="flex min-w-0 flex-col gap-2">
				<h3 class="text-sm font-semibold">{m.archive_axes_ai_step_preview()}</h3>
				<ul class="text-sm">
					{#each preview.axes as axis (axis.name)}
						<li>
							{m.archive_axes_ai_preview_axis({
								name: axis.name,
								matched: axis.matched,
								total: preview.itemCount
							})}
						</li>
					{/each}
				</ul>
				{#if preview.examples.length > 0}
					<p class="text-sm text-muted-foreground">{m.archive_axes_ai_preview_examples()}</p>
					<ul class="flex flex-col gap-2 text-sm">
						{#each preview.examples as example (example.relPath)}
							<li class="min-w-0">
								<p class="break-all">{example.title}</p>
								<p class="text-xs break-all text-muted-foreground">
									{preview.axes
										.map(
											(axis, i) =>
												`${axis.name}: ${example.values[i] ?? m.archive_axes_ai_preview_unset()}`
										)
										.join(' / ')}
								</p>
								<p class="text-xs break-all text-muted-foreground">{example.relPath}</p>
							</li>
						{/each}
					</ul>
				{/if}
			</section>
			{#if preview.replacesExisting}
				<WarningBand>
					<p class="text-sm leading-relaxed">{m.archive_axes_ai_replace_warning()}</p>
				</WarningBand>
			{/if}
		{/if}

		<Dialog.Footer>
			<Button variant="outline" onclick={() => (open = false)}>{m.action_cancel()}</Button>
			<LoadingButton loading={importing} disabled={!ready} onclick={importAxes}>
				{m.archive_axes_ai_import_button()}
			</LoadingButton>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
