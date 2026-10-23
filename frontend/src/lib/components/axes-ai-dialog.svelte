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
	/** 今出している画面。確かめた答えは別の画面 (preview) で見せる。 */
	let step = $state<'manual' | 'preview'>('manual');

	$effect(() => {
		// 開いたときだけ読み直す。loadPrompt の中で読む答えの欄を依存に入れると、入力のたびに消える。
		if (open) untrack(() => void loadPrompt());
		// 確かめている間も × とキャンセルでは閉じられる。その後に届いた答えで、予告の画面に進めない。
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
		step = 'manual';
		const current = ++session;
		const path = { params: { path: { id: contentId } } };
		try {
			const [levels, words, items] = await Promise.all([
				client.GET('/api/v1/contents/{id}/dir-levels', path),
				client.GET('/api/v1/contents/{id}/filename-words', path),
				client.GET('/api/v1/contents/{id}/items', path)
			]);
			if (current !== session) return;
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

	async function checkPasted() {
		await check();
		if (ready) step = 'preview';
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

	/** 取り込めなかったら、答えを貼った画面に戻ってそこで知らせる。 */
	function failImport(message: string, current: number) {
		if (current !== session) return;
		checked = null;
		step = 'manual';
		checkError = message;
	}

	async function importAxes() {
		if (!checked || !ready) return;
		const current = session;
		importing = true;
		try {
			const { error } = await client.PUT('/api/v1/contents/{id}/axes', {
				params: { path: { id: contentId } },
				body: checked.request
			});
			if (error) {
				failImport(answerErrorMessage(error), current);
				return;
			}
		} catch {
			failImport(GENERIC_ERROR_MESSAGE(), current);
			return;
		} finally {
			if (current === session) importing = false;
		}
		// 保存はできているので、読み直しに失敗しても取り込みは失敗と見せない。
		// 待つ間に閉じて開き直していたら、開き直した方は閉じない。
		if (current === session) open = false;
		toast.success(m.archive_axes_ai_imported_toast());
		await onimported().catch(() => errorDialog.show(GENERIC_ERROR_MESSAGE()));
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
			{#if step === 'manual'}
				<Dialog.Description>{m.archive_axes_ai_dialog_description()}</Dialog.Description>
			{/if}
		</Dialog.Header>
		<!-- 画面ごとに Dialog.Body を置き、替えたら新しい Body で先頭から見せる。1つを使い回すと、ブラウザーが
		     見えていた要素の位置を保つようにスクロールを合わせ (scroll anchoring)、替えた後の画面が途中から出るため。 -->
		{#if prompt === null && !promptFailed}
			<Dialog.Body>
				<Spinner aria-label={m.common_loading()} />
			</Dialog.Body>
		{:else if promptFailed}
			<Dialog.Body>
				<p role="alert" class="text-sm text-destructive">{m.archive_axes_ai_prompt_failed()}</p>
			</Dialog.Body>
		{:else if step === 'manual'}
			<Dialog.Body>
				<section class="flex min-w-0 flex-col gap-2">
					<h3 class="text-sm font-semibold">{m.archive_axes_ai_step_copy()}</h3>
					<p class="text-sm text-muted-foreground">{m.archive_axes_ai_privacy_note()}</p>
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
				</section>

				<section class="flex min-w-0 flex-col gap-2">
					<Field.Field>
						<Field.FieldLabel for="axes-ai-answer"
							>{m.archive_axes_ai_step_paste()}</Field.FieldLabel
						>
						<Textarea
							id="axes-ai-answer"
							bind:value={answer}
							rows={6}
							class="max-h-64 font-mono text-xs"
							disabled={checking}
						/>
					</Field.Field>
					{#if checkError}
						<p role="alert" class="text-sm text-destructive">{checkError}</p>
					{/if}
				</section>
			</Dialog.Body>
		{:else if checked}
			<Dialog.Body>
				{@const preview = checked.preview}
				<!-- 確定で起きることの警告は本体の先頭に置き、スクロールしなくても見せる (→ docs/ui.md「UI 全般」)。 -->
				{#if preview.replacesExisting}
					<WarningBand>
						<p class="text-sm leading-relaxed">{m.archive_axes_ai_replace_warning()}</p>
					</WarningBand>
				{/if}
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
			</Dialog.Body>
		{/if}

		<Dialog.Footer>
			{#if promptFailed}
				<Button variant="outline" onclick={() => (open = false)}>{m.action_cancel()}</Button>
			{:else if step === 'manual'}
				<Button variant="outline" onclick={() => (open = false)}>{m.action_cancel()}</Button>
				<LoadingButton
					loading={checking}
					disabled={answer.trim() === '' || prompt === null}
					onclick={checkPasted}
				>
					{m.archive_axes_ai_check_button()}
				</LoadingButton>
			{:else}
				<Button variant="outline" disabled={importing} onclick={() => (step = 'manual')}>
					{m.archive_axes_ai_back_button()}
				</Button>
				<LoadingButton loading={importing} disabled={!ready} onclick={importAxes}>
					{m.archive_axes_ai_import_button()}
				</LoadingButton>
			{/if}
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
