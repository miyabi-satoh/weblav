<script lang="ts">
	import { untrack } from 'svelte';
	import CheckIcon from '@lucide/svelte/icons/check';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import { toast } from 'svelte-sonner';
	import { client } from '$lib/api/client';
	import { errorCode, errorDetail, errorMessage, GENERIC_ERROR_MESSAGE } from '$lib/api/errors';
	import { fetchOrNull } from '$lib/api/load';
	import type { components } from '$lib/api/schema';
	import {
		buildAxesPrompt,
		parseAxesAnswer,
		type ImportAxesRequest,
		type ParseFailure
	} from '$lib/axes-ai-prompt';
	import { copyText } from '$lib/clipboard';
	import { CopiedState } from '$lib/copied-state.svelte';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { FormDirtyState } from '$lib/form-dirty.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import { Spinner } from '$lib/components/ui/spinner';
	import { Textarea } from '$lib/components/ui/textarea';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
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
	// 窓を閉じた後の読み直しの失敗を出す。
	const errorDialog = new ErrorDialogState();
	let checkError = $state<string | null>(null);
	/** WebLAV のアカウントを通して AI に送れるか (→ docs/ai.md「誰が使えるか」)。送れない人には貼り付けの流れだけを出す。 */
	let aiAvailable = $state(false);
	let generating = $state(false);
	let generateError = $state<string | null>(null);
	/** 開くたび・閉じるたびに進める。閉じた後に届いた応答を捨て、閉じた画面にも開き直した画面にも入れないため。 */
	let session = 0;
	/** 確かめた答えと、その予告。答えを書き換えたら取り込めないようにするため、両方を持つ。 */
	let checked = $state<{ answer: string; request: ImportAxesRequest; preview: Preview } | null>(
		null
	);
	let ready = $derived(checked !== null && checked.answer === answer);
	const copyFeedback = new CopiedState();
	// 貼った答えを、外側を押したり Esc で閉じたりして失わないようにする。
	const answerForm = new FormDirtyState(() => ({ answer }));
	/**
	 * 今出している画面。アカウントで送るか (choose)・手で頼むか (manual) を選び、確かめた答えは
	 * 別の画面 (preview) で見せる。1枚に並べると、送った人にも要らないプロンプトと答えの欄を通らせるため。
	 */
	let step = $state<'choose' | 'manual' | 'preview'>('manual');
	/** 予告の画面の「戻る」と、取り込みに失敗したときに戻る画面。 */
	let previewFrom = $state<'choose' | 'manual'>('manual');
	let contentEl = $state<HTMLElement | null>(null);

	// 画面を替えたら先頭から見せる。ブラウザーは押したボタン (足もとに残る) の位置を保つように
	// スクロールを合わせるので (scroll anchoring)、そのままだと替えた後の画面が途中から出るため。
	// 合わせるのは描いたときなので、描き終えてから戻す。
	$effect(() => {
		void step;
		const el = contentEl;
		if (!el) return;
		const frame = requestAnimationFrame(() => (el.scrollTop = 0));
		return () => cancelAnimationFrame(frame);
	});

	$effect(() => {
		// 開いたときだけ読み直す。loadPrompt の中で読む答えの欄を依存に入れると、入力のたびに消える。
		if (open) untrack(() => void loadPrompt());
		// 送っている間も × とキャンセルでは閉じられる。その後に届いた答えで、予告の問い合わせまで進めない。
		else session++;
	});

	async function loadPrompt() {
		prompt = null;
		promptFailed = false;
		answer = '';
		answerForm.markPristine();
		checked = null;
		checking = false;
		checkError = null;
		aiAvailable = false;
		step = 'manual';
		previewFrom = 'manual';
		generating = false;
		generateError = null;
		const current = ++session;
		const path = { params: { path: { id: contentId } } };
		try {
			const [levels, words, items, availability] = await Promise.all([
				client.GET('/api/v1/contents/{id}/dir-levels', path),
				client.GET('/api/v1/contents/{id}/filename-words', path),
				client.GET('/api/v1/contents/{id}/items', path),
				// 確かめられなくても、貼り付けの流れは使える。
				fetchOrNull(client.GET('/api/v1/ai/availability'))
			]);
			if (current !== session) return;
			aiAvailable = availability?.available ?? false;
			step = aiAvailable ? 'choose' : 'manual';
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

	/** プロンプトを AI に送り、返った答えを、貼り付けたときと同じ欄と確かめに流す。 */
	async function generate() {
		if (!prompt || generating) return;
		const current = session;
		generating = true;
		generateError = null;
		try {
			const { data, error } = await client.POST('/api/v1/contents/{id}/axes/ai/generate', {
				params: { path: { id: contentId } },
				body: { prompt }
			});
			if (current !== session) return;
			if (!data) {
				generateError = errorMessage(error);
				return;
			}
			answer = data.answer;
		} catch {
			if (current === session) generateError = GENERIC_ERROR_MESSAGE();
			return;
		} finally {
			if (current === session) generating = false;
		}
		await check();
		if (current !== session) return;
		if (checked) {
			showPreview('choose');
		} else {
			// AI の答えが読めないときは、送った画面で知らせる。答えの欄はこの画面に無いため。
			generateError = checkError;
			checkError = null;
		}
	}

	async function checkPasted() {
		await check();
		if (checked) showPreview('manual');
	}

	function showPreview(from: 'choose' | 'manual') {
		previewFrom = from;
		step = 'preview';
	}

	const PARSE_FAILURE_MESSAGES: Record<ParseFailure, () => string> = {
		notJson: m.archive_axes_ai_error_not_json,
		notAxes: m.archive_axes_ai_error_not_axes
	};

	async function check() {
		const current = answer;
		const currentSession = session;
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
			if (currentSession !== session) return;
			if (data) {
				checked = { answer: current, request: parsed.value, preview: data };
			} else {
				checkError = answerErrorMessage(error);
			}
		} catch {
			if (currentSession === session) checkError = GENERIC_ERROR_MESSAGE();
		} finally {
			if (currentSession === session) checking = false;
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

	/** 取り込めなかったら、答えを作った画面に戻ってそこで知らせる。 */
	function failImport(message: string) {
		checked = null;
		step = previewFrom;
		if (previewFrom === 'choose') generateError = message;
		else checkError = message;
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
				failImport(answerErrorMessage(error));
				return;
			}
		} catch {
			failImport(GENERIC_ERROR_MESSAGE());
			return;
		} finally {
			importing = false;
		}
		// 保存はできているので、読み直しに失敗しても取り込みは失敗と見せない。
		open = false;
		toast.success(m.archive_axes_ai_imported_toast());
		await onimported().catch(() => errorDialog.show(GENERIC_ERROR_MESSAGE()));
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content
		bind:ref={contentEl}
		class="sm:max-w-2xl"
		interactOutsideBehavior={answerForm.dirty || generating ? 'ignore' : 'close'}
		escapeKeydownBehavior={answerForm.dirty || generating ? 'ignore' : 'close'}
	>
		<Dialog.Header>
			<Dialog.Title>{m.archive_axes_ai_dialog_title()}</Dialog.Title>
			{#if step === 'manual'}
				<Dialog.Description>{m.archive_axes_ai_dialog_description()}</Dialog.Description>
			{/if}
		</Dialog.Header>

		{#if prompt === null && !promptFailed}
			<Spinner aria-label={m.common_loading()} />
		{:else if step === 'choose'}
			<!-- 2つの作り方を、同じ形の枠に並べて選ばせる (→ docs/ui.md「UI 全般」の「余白と区切り」)。 -->
			<section class="flex min-w-0 flex-col gap-2 rounded-md border p-4">
				<h3 class="text-sm font-semibold">{m.archive_axes_ai_generate_heading()}</h3>
				<p class="text-sm text-muted-foreground">{m.archive_axes_ai_generate_note()}</p>
				<LoadingButton
					class="w-fit"
					loading={generating || checking}
					disabled={prompt === null}
					onclick={generate}
				>
					{m.archive_axes_ai_generate_button()}
				</LoadingButton>
				{#if generating}
					<p class="text-sm text-muted-foreground">{m.archive_axes_ai_generate_waiting()}</p>
				{/if}
				{#if generateError}
					<p role="alert" class="text-sm text-destructive">{generateError}</p>
				{/if}
			</section>
			<section class="flex min-w-0 flex-col gap-2 rounded-md border p-4">
				<h3 class="text-sm font-semibold">{m.archive_axes_ai_manual_heading()}</h3>
				<p class="text-sm text-muted-foreground">{m.archive_axes_ai_dialog_description()}</p>
				<Button
					variant="outline"
					class="w-fit"
					disabled={generating || checking}
					onclick={() => (step = 'manual')}
				>
					{m.archive_axes_ai_manual_button()}
				</Button>
			</section>
		{:else if step === 'manual'}
			<section class="flex min-w-0 flex-col gap-2">
				<h3 class="text-sm font-semibold">{m.archive_axes_ai_step_copy()}</h3>
				<p class="text-sm text-muted-foreground">{m.archive_axes_ai_privacy_note()}</p>
				{#if prompt !== null}
					<Button variant="outline" class="w-fit" onclick={copyPrompt}>
						{#if copyFeedback.copied}
							<CheckIcon />
						{:else}
							<CopyIcon />
						{/if}
						{m.archive_axes_ai_copy_button()}
					</Button>
					<!-- 中身は畳んでおく。開いたままだと、ダイアログの途中でそれ自体がスクロールし、ホイールを取られるため。 -->
					<details class="text-sm">
						<summary class="w-fit cursor-pointer text-muted-foreground">
							{m.archive_axes_ai_prompt_show()}
						</summary>
						<pre
							class="mt-2 max-h-48 overflow-auto rounded-md border p-2 font-mono text-xs whitespace-pre-wrap">{prompt}</pre>
					</details>
				{:else}
					<p role="alert" class="text-sm text-destructive">{m.archive_axes_ai_prompt_failed()}</p>
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
				{#if checkError}
					<p role="alert" class="text-sm text-destructive">{checkError}</p>
				{/if}
			</section>
		{:else if checked}
			{@const preview = checked.preview}
			<section class="flex min-w-0 flex-col gap-2">
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
			{#if step === 'manual'}
				{#if aiAvailable}
					<Button variant="outline" onclick={() => (step = 'choose')}>
						{m.archive_axes_ai_back_button()}
					</Button>
				{:else}
					<Button variant="outline" onclick={() => (open = false)}>{m.action_cancel()}</Button>
				{/if}
				<LoadingButton
					loading={checking}
					disabled={answer.trim() === '' || prompt === null}
					onclick={checkPasted}
				>
					{m.archive_axes_ai_check_button()}
				</LoadingButton>
			{:else if step === 'preview'}
				<Button variant="outline" disabled={importing} onclick={() => (step = previewFrom)}>
					{m.archive_axes_ai_back_button()}
				</Button>
				<LoadingButton loading={importing} disabled={!ready} onclick={importAxes}>
					{m.archive_axes_ai_import_button()}
				</LoadingButton>
			{:else}
				<Button variant="outline" onclick={() => (open = false)}>{m.action_cancel()}</Button>
			{/if}
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
