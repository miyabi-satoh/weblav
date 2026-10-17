<script lang="ts">
	import { untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { invalidateAll } from '$app/navigation';
	import { client } from '$lib/api/client';
	import type { components } from '$lib/api/schema';
	import { MAX_INTEREST_UNITS } from '$lib/ai-links';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { FormDirtyState } from '$lib/form-dirty.svelte';
	import { sectionCardClass } from '$lib/page-layout';
	import * as m from '$lib/paraglide/messages.js';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import * as RadioGroup from '$lib/components/ui/radio-group';
	import { Switch } from '$lib/components/ui/switch';
	import { Textarea } from '$lib/components/ui/textarea';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import IntegerInput from '$lib/components/integer-input.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';

	type Settings = components['schemas']['AiLinksSettings'];
	type AutoMode = components['schemas']['AutoMode'];

	// グループの編集ページの「AI でリンクを収集」の区画 (→ docs/ai.md「AI が集めるリンク」)。
	// 区画ごとに保存する (→ docs/ui.md「UI 全般」の「サイト設定の区画」)。
	let {
		contentId,
		settings,
		editable,
		ondirtychange
	}: {
		contentId: number;
		settings: Settings;
		editable: boolean;
		/** 保存していない変更の有無が変わったとき。ページが、離れるときに確認を出すかを決めるのに使う。 */
		ondirtychange: (dirty: boolean) => void;
	} = $props();

	/** 1日の回数と自動の更新の時間の上限。サーバーが受ける数 (`u32`) の上限で、利用者に選ばせる値の目安ではない。 */
	const MAX_COUNT = 4_294_967_295;
	/** 自動の更新を選び直したときに入れておく値。 */
	const DEFAULT_HOURS = 6;
	const DEFAULT_TIME = '07:00';

	let on = $state(untrack(() => settings.enabled));
	let interests = $state(untrack(() => settings.interests ?? ''));
	let dailyLimit = $state(untrack(() => settings.dailyLimit));
	let mode = $state<AutoMode>(untrack(() => settings.autoRefresh.mode));
	let hours = $state(untrack(() => settings.autoRefresh.hours ?? DEFAULT_HOURS));
	let time = $state(untrack(() => settings.autoRefresh.time ?? DEFAULT_TIME));

	const form = new FormDirtyState(() => ({ on, interests, dailyLimit, mode, hours, time }));
	form.markPristine();
	$effect(() => {
		ondirtychange(form.dirty);
	});

	// 空白だけでは `required` を通ってしまうので、保存ボタンの側で止める。長さは欄の maxlength で止める。
	let interestsMissing = $derived(on && interests.trim() === '');

	let saving = $state(false);
	const errorDialog = new ErrorDialogState();

	async function save(event: SubmitEvent) {
		event.preventDefault();
		if (saving || interestsMissing) return;
		saving = true;
		try {
			await errorDialog.attempt(
				() =>
					client.PUT('/api/v1/contents/{id}/ai-links/settings', {
						params: { path: { id: contentId } },
						body: on
							? {
									interests,
									dailyLimit,
									autoRefresh: {
										mode,
										hours: mode === 'interval' ? hours : null,
										time: mode === 'daily' ? time : null
									}
								}
							: { interests: null }
					}),
				async () => {
					form.markPristine();
					toast.success(m.contents_edit_saved_toast());
					// 表示しないものの区画を、オンにした・外した結果に合わせて出し直す。
					await invalidateAll();
				}
			);
		} finally {
			saving = false;
		}
	}

	/** 変更を捨てる。離れるときの確認で「破棄」を選んだときに呼ぶ。 */
	export function discard() {
		form.markPristine();
	}
</script>

<form onsubmit={save} aria-labelledby="ai-links-section-heading" class={sectionCardClass}>
	<fieldset disabled={!editable} class="contents">
		<Field.Field orientation="horizontal">
			<Field.FieldContent>
				<h2 id="ai-links-section-heading" class="text-base font-semibold">
					<label for="content-ai-links">{m.contents_form_ai_links_label()}</label>
				</h2>
				<Field.FieldDescription>{m.contents_form_ai_links_description()}</Field.FieldDescription>
			</Field.FieldContent>
			<Switch id="content-ai-links" bind:checked={on} disabled={!editable} />
		</Field.Field>
		{#if on}
			<Field.FieldGroup>
				<Field.Field>
					<Field.FieldLabel for="content-ai-interests" required
						>{m.contents_form_ai_interests_label()}</Field.FieldLabel
					>
					<Textarea
						id="content-ai-interests"
						bind:value={interests}
						maxlength={MAX_INTEREST_UNITS}
						required
					/>
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel for="content-ai-daily-limit" required
						>{m.contents_form_ai_daily_limit_label()}</Field.FieldLabel
					>
					<IntegerInput
						id="content-ai-daily-limit"
						class="w-44"
						min={0}
						max={MAX_COUNT}
						unit={m.contents_form_ai_daily_limit_unit({ count: dailyLimit })}
						bind:value={dailyLimit}
						required
					/>
					<Field.FieldDescription
						>{m.contents_form_ai_daily_limit_description()}</Field.FieldDescription
					>
				</Field.Field>
				<Field.FieldSet>
					<Field.FieldLegend variant="label" id="content-ai-auto-legend"
						>{m.contents_form_ai_auto_label()}</Field.FieldLegend
					>
					<RadioGroup.Root bind:value={mode} aria-labelledby="content-ai-auto-legend">
						<div class="flex min-h-9 items-center gap-2">
							<RadioGroup.Item value="off" id="content-ai-auto-off" />
							<label for="content-ai-auto-off" class="text-sm"
								>{m.contents_form_ai_auto_off()}</label
							>
						</div>
						<!-- 数の欄を文の途中に置く。押せる範囲を丸だけでなく文の言葉にも広げる。 -->
						<div class="flex min-h-9 flex-wrap items-center gap-2">
							<RadioGroup.Item value="interval" id="content-ai-auto-interval" />
							<label for="content-ai-auto-interval" class="text-sm"
								>{m.contents_form_ai_auto_interval_before()}</label
							>
							<IntegerInput
								id="content-ai-auto-hours"
								class="w-44"
								min={1}
								max={MAX_COUNT}
								unit={m.contents_form_ai_auto_hours_unit({ count: hours })}
								label={m.contents_form_ai_auto_hours_unit({ count: hours })}
								bind:value={hours}
								required={mode === 'interval'}
							/>
							<label for="content-ai-auto-interval" class="text-sm"
								>{m.contents_form_ai_auto_interval_after()}</label
							>
						</div>
						<div class="flex min-h-9 flex-wrap items-center gap-2">
							<RadioGroup.Item value="daily" id="content-ai-auto-daily" />
							<label for="content-ai-auto-daily" class="text-sm"
								>{m.contents_form_ai_auto_daily_before()}</label
							>
							<Input
								id="content-ai-auto-time"
								type="time"
								class="w-40"
								aria-label={m.contents_form_ai_auto_time_label()}
								bind:value={time}
								required={mode === 'daily'}
							/>
							{#if m.contents_form_ai_auto_daily_after() !== ''}
								<label for="content-ai-auto-daily" class="text-sm"
									>{m.contents_form_ai_auto_daily_after()}</label
								>
							{/if}
						</div>
					</RadioGroup.Root>
					<Field.FieldDescription>{m.contents_form_ai_auto_description()}</Field.FieldDescription>
				</Field.FieldSet>
			</Field.FieldGroup>
		{/if}
	</fieldset>
	{#if editable}
		<div>
			<LoadingButton type="submit" loading={saving} disabled={!form.dirty || interestsMissing}
				>{m.action_save()}</LoadingButton
			>
		</div>
	{/if}
</form>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
