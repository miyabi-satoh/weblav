<script lang="ts">
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import { page } from '$app/state';
	import { client } from '$lib/api/client';
	import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
	import { fetchOrNull } from '$lib/api/load';
	import * as m from '$lib/paraglide/messages.js';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import { Textarea } from '$lib/components/ui/textarea';
	import LoadingButton from '$lib/components/loading-button.svelte';

	// 簡易チャット (→ docs/ai.md「簡易チャット」)。どの画面からでも開けるよう、画面の右下に浮かせる。
	// 質問を1つ送り、答えを1つ受け取る。前のやり取りは持たない。
	let {
		bottomOffset = 0
	}: {
		/** 下に固定した音声のプレイヤーの高さ。ボタンがプレイヤーに重ならないよう、その分だけ上げる。 */
		bottomOffset?: number;
	} = $props();

	// AI を使える人にだけボタンを出す。ログインし直したら確かめ直す。
	let available = $state(false);
	let userId = $derived(page.data.user?.id ?? null);
	$effect(() => {
		available = false;
		if (userId === null) return;
		let cancelled = false;
		fetchOrNull(client.GET('/api/v1/ai/availability')).then((ai) => {
			if (!cancelled) available = ai?.available ?? false;
		});
		return () => {
			cancelled = true;
		};
	});

	let open = $state(false);
	let prompt = $state('');
	let sending = $state(false);
	let answer = $state<string | null>(null);
	let failure = $state<string | null>(null);

	async function send(event?: SubmitEvent) {
		event?.preventDefault();
		if (sending || prompt.trim() === '') return;
		sending = true;
		failure = null;
		try {
			const { data, error } = await client.POST('/api/v1/ai/chat', { body: { prompt } });
			if (data) answer = data.answer;
			else failure = errorMessage(error);
		} catch {
			failure = GENERIC_ERROR_MESSAGE();
		} finally {
			sending = false;
		}
	}

	// 改行は Enter のまま、送るのは Ctrl+Enter (macOS は ⌘+Enter)。長めの質問を書けるように。
	function handleKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter' && (event.ctrlKey || event.metaKey) && !event.isComposing) {
			event.preventDefault();
			void send();
		}
	}
</script>

{#if available}
	<Button
		size="icon-lg"
		class="fixed right-4 z-20 rounded-full shadow-lg md:right-6"
		style="bottom: {bottomOffset + 16}px"
		onclick={() => (open = true)}
	>
		<SparklesIcon />
		<span class="sr-only">{m.ai_chat_open_button()}</span>
	</Button>

	<Dialog.Root bind:open>
		<Dialog.Content>
			<Dialog.Header>
				<Dialog.Title>{m.ai_chat_title()}</Dialog.Title>
				<Dialog.Description>{m.ai_chat_description()}</Dialog.Description>
			</Dialog.Header>
			<form onsubmit={send} class="flex flex-col gap-4">
				<Field.Field>
					<Field.FieldLabel for="ai-chat-prompt">{m.ai_chat_prompt_label()}</Field.FieldLabel>
					<Textarea id="ai-chat-prompt" bind:value={prompt} onkeydown={handleKeydown} required />
				</Field.Field>
				<div>
					<LoadingButton type="submit" loading={sending} disabled={prompt.trim() === ''}>
						{m.ai_chat_send_button()}
					</LoadingButton>
				</div>
			</form>
			<div aria-live="polite" class="flex flex-col gap-2">
				{#if failure !== null}
					<p class="text-sm text-destructive">{failure}</p>
				{:else if answer !== null}
					<p class="text-sm font-semibold">{m.ai_chat_answer_label()}</p>
					<p class="max-h-80 overflow-y-auto text-sm wrap-anywhere whitespace-pre-wrap">{answer}</p>
				{/if}
			</div>
		</Dialog.Content>
	</Dialog.Root>
{/if}
