<script lang="ts">
	/**
	 * 「パターンに追加...」(→ docs/ui.md「パターンに追加」)。名前の絞り込みの語から候補を作り、追加先を選ばせ、
	 * 変わる件数を見せてから、その場で保存する。窓の後ろで一覧が変わるので、知らせ (トースト) は出さない。
	 */
	import { untrack } from 'svelte';
	import { client } from '$lib/api/client';
	import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
	import type { components } from '$lib/api/schema';
	import { ContentFormState } from '$lib/content-form.svelte';
	import { dialogFormClass } from '$lib/dialog-form';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { formatNumber } from '$lib/format';
	import { LatestRequest } from '$lib/latest-request';
	import * as m from '$lib/paraglide/messages.js';
	import { appendPattern, patternCandidate } from '$lib/pattern-candidate';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import * as RadioGroup from '$lib/components/ui/radio-group';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';

	type Content = components['schemas']['AdminContentResponse'];
	type VisibilityPreview = components['schemas']['VisibilityPreview'];
	type Target = 'exclude' | 'include';

	/** 打ち止めてから数えるまでの間。数えるのに全ファイルを読むので、打つたびには数えない。 */
	const COUNT_DELAY_MS = 300;

	let {
		open = $bindable(),
		content,
		filterText,
		initialTarget = 'exclude',
		note,
		onsaved
	}: {
		open: boolean;
		content: Content;
		/** 名前の絞り込みの語。候補の元にする。 */
		filterText: string;
		/** 開いたときの追加先。 */
		initialTarget?: Target;
		/** 窓の中に添える案内。 */
		note?: string;
		/** 保存した後の画面の更新。 */
		onsaved: () => void | Promise<void>;
	} = $props();

	let target = $state<Target>('exclude');
	let pattern = $state('');
	let counting = $state(false);
	let preview = $state<VisibilityPreview | null>(null);
	/** 正しくないパターンの文言。出ている間は追加できない。 */
	let invalid = $state<string | null>(null);
	let saving = $state(false);
	const errorDialog = new ErrorDialogState();
	const requests = new LatestRequest();

	let trimmed = $derived(pattern.trim());
	/** 見えるものが変わらない。足しても意味が無いので、追加できない。 */
	let noChange = $derived(
		preview !== null && preview.gained === 0 && preview.lost === 0 && !preview.truncated
	);

	$effect.pre(() => {
		if (!open) return;
		const text = filterText;
		const firstTarget = initialTarget;
		untrack(() => {
			target = firstTarget;
			pattern = patternCandidate(text);
		});
	});

	/** 保存するパターン。追加先の方だけに足す。 */
	function patternsWith(source: Content, candidate: string, to: Target) {
		return {
			includePatterns:
				to === 'include'
					? appendPattern(source.includePatterns, candidate)
					: (source.includePatterns ?? null),
			excludePatterns:
				to === 'exclude'
					? appendPattern(source.excludePatterns, candidate)
					: (source.excludePatterns ?? null)
		};
	}

	$effect(() => {
		const candidate = trimmed;
		const to = target;
		const source = content;
		const isCurrent = requests.begin();
		preview = null;
		invalid = null;
		if (!open || candidate === '') {
			counting = false;
			return;
		}
		counting = true;
		// 打ち直したら前の問い合わせをやめる。結果を使わないものが重ならないように。
		const controller = new AbortController();
		const timer = setTimeout(async () => {
			try {
				const result = await client.POST('/api/v1/contents/{id}/visibility-preview', {
					params: { path: { id: source.id } },
					body: patternsWith(source, candidate, to),
					signal: controller.signal
				});
				if (!isCurrent()) return;
				if (result.response.status === 422) invalid = errorMessage(result.error);
				preview = result.data ?? null;
			} catch {
				// 数えられなくても追加はできる。件数が出ないだけ。
			} finally {
				if (isCurrent()) counting = false;
			}
		}, COUNT_DELAY_MS);
		return () => {
			clearTimeout(timer);
			controller.abort();
		};
	});

	async function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (saving || trimmed === '' || invalid !== null || noChange) return;
		saving = true;
		try {
			// 更新の API は全項目の置き換えなので、一覧を読み込んだ後に別の人が変えた項目を古い値で戻さない。
			const latest = await client.GET('/api/v1/contents/manage');
			const fresh = latest.data?.find((item) => item.id === content.id);
			if (!latest.response.ok || !fresh) {
				errorDialog.show(errorMessage(latest.error));
				return;
			}
			const form = new ContentFormState();
			form.loadFrom(fresh, false);
			const next = patternsWith(fresh, trimmed, target);
			form.includePatterns = next.includePatterns ?? '';
			form.excludePatterns = next.excludePatterns ?? '';
			const result = await form.replace(content.id);
			if (!result.response.ok) {
				errorDialog.show(errorMessage(result.error));
				return;
			}
			await onsaved();
			open = false;
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			saving = false;
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content
		escapeKeydownBehavior={saving ? 'ignore' : 'close'}
		interactOutsideBehavior={saving ? 'ignore' : 'close'}
	>
		<Dialog.Header>
			<Dialog.Title>{m.pattern_add_title()}</Dialog.Title>
		</Dialog.Header>
		<form onsubmit={handleSubmit} class={dialogFormClass}>
			<Dialog.Body>
				<Field.FieldGroup>
					<div class="flex flex-col gap-2">
						<p id="pattern-add-target" class="font-medium">{m.pattern_add_target_label()}</p>
						<RadioGroup.Root bind:value={target} aria-labelledby="pattern-add-target">
							{#each ['exclude', 'include'] as const as option (option)}
								<Field.Field orientation="horizontal" class="min-h-9">
									<RadioGroup.Item value={option} id="pattern-add-{option}" />
									<Field.FieldLabel for="pattern-add-{option}" class="font-normal">
										{option === 'exclude'
											? m.pattern_add_target_exclude()
											: m.pattern_add_target_include()}
									</Field.FieldLabel>
								</Field.Field>
							{/each}
						</RadioGroup.Root>
						{#if note}
							<Field.FieldDescription>{note}</Field.FieldDescription>
						{/if}
					</div>
					<Field.Field data-invalid={invalid !== null}>
						<Field.FieldLabel for="pattern-add-pattern" required>
							{m.pattern_add_pattern_label()}
						</Field.FieldLabel>
						<Input
							id="pattern-add-pattern"
							class="font-mono"
							autocomplete="off"
							spellcheck="false"
							required
							aria-invalid={invalid !== null}
							bind:value={pattern}
						/>
						{#if invalid !== null}
							<Field.FieldError>{invalid}</Field.FieldError>
						{/if}
						<Field.FieldDescription>{m.pattern_add_pattern_help()}</Field.FieldDescription>
					</Field.Field>
					<!-- 絞り込みはタイトルに当たりパターンはパスに当たるので、候補が1件も当たらないことがある。ここで気づける。 -->
					<div role="status" class="text-sm">
						{#if counting}
							<p class="text-muted-foreground">{m.pattern_add_counting()}</p>
						{:else if invalid !== null || trimmed === ''}
							<!-- 数えるものが無い。 -->
						{:else if preview === null}
							<p>{m.contents_edit_visibility_change_unknown()}</p>
						{:else if noChange}
							<p>{m.pattern_add_no_change()}</p>
						{:else}
							{#if preview.gained > 0}
								<p>
									{m.contents_edit_visibility_change_gained({
										count: formatNumber(preview.gained)
									})}
								</p>
							{/if}
							{#if preview.lost > 0}
								<p>
									{m.contents_edit_visibility_change_lost({ count: formatNumber(preview.lost) })}
								</p>
							{/if}
							{#if preview.truncated}
								<p class="text-muted-foreground">{m.contents_edit_visibility_change_truncated()}</p>
							{/if}
						{/if}
					</div>
				</Field.FieldGroup>
			</Dialog.Body>
			<Dialog.Footer>
				<Button type="button" variant="outline" disabled={saving} onclick={() => (open = false)}>
					{m.action_cancel()}
				</Button>
				<LoadingButton
					type="submit"
					loading={saving}
					disabled={trimmed === '' || counting || invalid !== null || noChange}
				>
					{m.action_add()}
				</LoadingButton>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
