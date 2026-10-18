<script lang="ts">
	import PaperclipIcon from '@lucide/svelte/icons/paperclip';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import XIcon from '@lucide/svelte/icons/x';
	import { tick, untrack } from 'svelte';
	import { page } from '$app/state';
	import { AI_CHAT_PANEL_ID, aiChat, type AiChatFile } from '$lib/ai-chat.svelte';
	import { client } from '$lib/api/client';
	import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
	import { fetchOrNull } from '$lib/api/load';
	import { formatByteSize } from '$lib/format';
	import * as m from '$lib/paraglide/messages.js';
	import { Button } from '$lib/components/ui/button';
	import * as Field from '$lib/components/ui/field';
	import { Textarea } from '$lib/components/ui/textarea';
	import LoadingButton from '$lib/components/loading-button.svelte';

	// 簡易チャット (→ docs/ai.md「簡易チャット」)。どの画面からでも開けるよう、画面の右下に浮かせる。
	// 質問を1つ送り、答えを1つ受け取る。前のやり取りは持たない。
	// 開くとページやビューアーに重ねず横 (狭い幅では下) に並べ、見ながら・写しながら聞けるようにする (→ docs/ui.md「AI に聞く」)。
	let {
		bottomOffset = 0
	}: {
		/** 下に固定した音声のプレイヤーの高さ。ボタンがプレイヤーに重ならないよう、その分だけ上げる。 */
		bottomOffset?: number;
	} = $props();

	/** 下の段の高さの範囲 (画面の高さに対する割合)。上にページかビューアーを残し、下にも答えを読める分を残す。 */
	const SHEET_MIN = 0.25;
	const SHEET_MAX = 0.85;
	/** 矢印キーで高さを変える1回の幅。 */
	const SHEET_STEP = 0.05;
	/** 付けられるファイルの大きさ。窓口が受ける添付の上限 (base64 で 1,400 万文字) を元の大きさに直したもの。 */
	const MAX_FILE_BYTES = (14_000_000 / 4) * 3;

	// AI を使える人にだけボタンを出す。ログインし直したときと、コンピューターの登録が変わったときに確かめ直す。
	let userId = $derived(page.data.user?.id ?? null);
	// 前の人の質問と答えは、次の人に見せない。
	$effect(() => {
		void userId;
		aiChat.available = false;
		prompt = '';
		answer = null;
		failure = null;
	});
	$effect(() => {
		void aiChat.checks;
		if (userId === null) {
			aiChat.available = false;
			return;
		}
		let cancelled = false;
		fetchOrNull(client.GET('/api/v1/ai/availability')).then((ai) => {
			if (!cancelled) aiChat.available = ai?.available ?? false;
		});
		return () => {
			cancelled = true;
		};
	});

	let prompt = $state('');
	let sending = $state(false);
	let answer = $state<string | null>(null);
	/** 答えが、どのファイルについてのものか。ファイルを付けずに聞いたなら `null`。 */
	let answerAbout = $state<string | null>(null);
	/**
	 * 見ているファイルを付けるか (→ docs/ui.md「AI に聞く」)。最初から付け、× で外す。
	 * 外したのは、そのファイルの間だけ。別のファイルに移ったら付け直す。
	 */
	let detached = $state(false);
	$effect.pre(() => {
		void aiChat.file?.src;
		detached = false;
	});
	let attached = $derived(detached ? null : aiChat.file);
	let failure = $state<string | null>(null);
	let promptInput = $state<HTMLTextAreaElement | null>(null);
	let openButton = $state<HTMLButtonElement | null>(null);
	/** 下の段の高さ (画面の高さに対する割合)。 */
	let sheet = $state(0.5);

	// 開いている間、パネルの分だけ画面を空ける (→ routes/layout.css の `ai-panel-clearance`)。
	// 開いたら質問の欄へ移り、閉じたら開いたボタンへ戻す。
	//
	// ビューアー (bits-ui の Dialog・PhotoSwipe) は、外へ出たフォーカスを document の focusin で引き戻す。
	// パネルの中の focusin は window の捕捉の段で止め、そこまで届かせない。質問の欄へ移す前に付ける。
	// FIX: bits-ui の Dialog の trapFocus は開いた時点でしか効かず、後から外しても引き戻し続けるため、ビューアー側では止められない。
	$effect(() => {
		if (!aiChat.open) return;
		const keepFocus = (event: FocusEvent) => {
			if (aiChat.contains(event.target)) event.stopPropagation();
		};
		window.addEventListener('focusin', keepFocus, true);
		const root = document.documentElement;
		root.toggleAttribute('data-ai-panel-open', true);
		const opener = document.activeElement;
		untrack(() => promptInput)?.focus();
		return () => {
			window.removeEventListener('focusin', keepFocus, true);
			root.removeAttribute('data-ai-panel-open');
			// 開いたボタンが無ければ (右下のボタンはパネルを開くと消え、閉じると出し直す)、開いているビューアーか、
			// 出し直した右下のボタンへ戻す。ビューアーの外にフォーカスがあると、Esc や左右キーがビューアーに届かないため。
			void tick().then(() => {
				const target =
					opener instanceof HTMLElement && opener.isConnected
						? opener
						: (document.querySelector<HTMLElement>('[role="dialog"], .pswp') ?? openButton);
				target?.focus();
			});
		};
	});

	$effect(() => {
		document.documentElement.style.setProperty('--ai-sheet-height', `${sheet * 100}svh`);
	});

	async function send(event?: SubmitEvent) {
		event?.preventDefault();
		if (sending || prompt.trim() === '') return;
		sending = true;
		const asker = userId;
		// 前の答えを残すと、新しい質問の答えに見えるため。
		answer = null;
		failure = null;
		const about = attached;
		try {
			const file = about === null ? undefined : await fileOf(about);
			if (file === null) {
				failure = m.ai_chat_file_too_large({ max: formatByteSize(MAX_FILE_BYTES) });
				return;
			}
			const { data, error } = await client.POST('/api/v1/ai/chat', { body: { prompt, file } });
			// 待つ間に人が替わったら、前の人の答えを出さない。
			if (userId !== asker) return;
			if (data) {
				answer = data.answer;
				answerAbout = about?.name ?? null;
			} else failure = errorMessage(error);
		} catch {
			if (userId === asker) failure = GENERIC_ERROR_MESSAGE();
		} finally {
			sending = false;
		}
	}

	/**
	 * 付けるファイルを、送る形にする。中身はビューアーが開いている URL から、見ている人の権限で取る。
	 * 大きすぎて送れなければ `null`。
	 */
	async function fileOf(
		file: AiChatFile
	): Promise<{ name: string; mimeType?: string; data?: string } | null> {
		if (!file.sendsContent) return { name: file.name };
		const response = await fetch(file.src);
		if (!response.ok) throw new Error(`failed to read ${file.src}`);
		// 大きさが分かれば、取り終える前に断る。大きなファイルを丸ごとメモリに読まないように。
		if (Number(response.headers.get('content-length') ?? 0) > MAX_FILE_BYTES) {
			await response.body?.cancel();
			return null;
		}
		const blob = await response.blob();
		if (blob.size > MAX_FILE_BYTES) return null;
		return { name: file.name, mimeType: blob.type || undefined, data: await base64Of(blob) };
	}

	function base64Of(blob: Blob): Promise<string> {
		return new Promise((resolve, reject) => {
			const reader = new FileReader();
			// data URL (`data:<種類>;base64,<中身>`) の中身だけを使う。
			reader.onload = () => resolve(String(reader.result).replace(/^[^,]*,/, ''));
			reader.onerror = () => reject(reader.error);
			reader.readAsDataURL(blob);
		});
	}

	// 改行は Enter のまま、送るのは Ctrl+Enter (macOS は ⌘+Enter)。長めの質問を書けるように。
	function handlePromptKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter' && (event.ctrlKey || event.metaKey) && !event.isComposing) {
			event.preventDefault();
			void send();
		}
	}

	// パネルの中のキー操作を、後ろのビューアーに渡さない。ビューアーは document でキーを受け、
	// Esc で閉じ、左右キーで前後のファイルへ移るため、質問の欄で打った矢印や Esc で動いてしまう。
	// Esc はパネルを閉じる。
	function handlePanelKeydown(event: KeyboardEvent) {
		event.stopPropagation();
		if (event.key === 'Escape' && !event.isComposing) {
			event.preventDefault();
			aiChat.open = false;
		}
	}

	function clampSheet(value: number): number {
		return Math.min(SHEET_MAX, Math.max(SHEET_MIN, value));
	}

	function handleGripPointerdown(event: PointerEvent) {
		if (event.button !== 0) return;
		const grip = event.currentTarget as HTMLElement;
		grip.setPointerCapture(event.pointerId);
		const move = (e: PointerEvent) => {
			sheet = clampSheet(1 - e.clientY / window.innerHeight);
		};
		const end = () => {
			grip.removeEventListener('pointermove', move);
			grip.removeEventListener('pointerup', end);
			grip.removeEventListener('pointercancel', end);
		};
		grip.addEventListener('pointermove', move);
		grip.addEventListener('pointerup', end);
		grip.addEventListener('pointercancel', end);
	}

	function handleGripKeydown(event: KeyboardEvent) {
		if (event.key === 'ArrowUp') sheet = clampSheet(sheet + SHEET_STEP);
		else if (event.key === 'ArrowDown') sheet = clampSheet(sheet - SHEET_STEP);
		else return;
		event.preventDefault();
	}
</script>

{#if aiChat.available && !aiChat.open}
	<Button
		bind:ref={openButton}
		size="icon-lg"
		class="fixed right-4 z-20 rounded-full shadow-lg md:right-6"
		style="bottom: {bottomOffset + 16}px"
		onclick={() => (aiChat.open = true)}
	>
		<SparklesIcon />
		<span class="sr-only">{m.ai_chat_open_button()}</span>
	</Button>
{/if}

{#if aiChat.open}
	<!-- ビューアー (z-50) を開いていても、その横に出す。ビューアーが開いている間は body が押せなくなる
	     (bits-ui の Dialog) ので、パネルだけ押せるようにする。
	     キーは中の欄やボタンから上がってきたものを受けるだけで、この要素自体はフォーカスを受けない。 -->
	<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
	<aside
		id={AI_CHAT_PANEL_ID}
		aria-labelledby="ai-chat-title"
		class="pointer-events-auto fixed inset-x-0 bottom-0 z-60 flex ai-sheet-height flex-col border-t bg-background text-foreground shadow-lg lg:inset-y-0 lg:right-0 lg:left-auto lg:w-96 lg:border-t-0 lg:border-l"
		onkeydown={handlePanelKeydown}
	>
		<!-- 狭い幅では、引っ張って下の段の高さを変える。フォーカスを受けて上下キーで動かせる区切り
		     (WAI-ARIA の Window Splitter) で、Svelte は separator を操作の要素と見なさないので警告を止める。 -->
		<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
		<div
			role="separator"
			aria-orientation="horizontal"
			aria-label={m.ai_chat_resize_label()}
			aria-valuemin={SHEET_MIN * 100}
			aria-valuemax={SHEET_MAX * 100}
			aria-valuenow={Math.round(sheet * 100)}
			tabindex="0"
			class="flex h-6 shrink-0 cursor-row-resize touch-none items-center justify-center outline-none focus-visible:ring-3 focus-visible:ring-ring/50 lg:hidden"
			onpointerdown={handleGripPointerdown}
			onkeydown={handleGripKeydown}
		>
			<span class="h-1 w-10 rounded-full bg-border"></span>
		</div>
		<div class="flex shrink-0 items-start gap-2 px-4 pb-2 lg:pt-4">
			<div class="flex min-w-0 flex-1 flex-col gap-1">
				<h2 id="ai-chat-title" class="text-base font-semibold">{m.ai_chat_title()}</h2>
				<p class="text-sm text-muted-foreground">{m.ai_chat_description()}</p>
			</div>
			<Button variant="ghost" size="icon" onclick={() => (aiChat.open = false)}>
				<XIcon />
				<span class="sr-only">{m.action_close()}</span>
			</Button>
		</div>
		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 pb-4">
			<form onsubmit={send} class="flex flex-col gap-4">
				{#if aiChat.file}
					<div class="flex flex-col gap-1">
						{#if attached}
							<div
								class="flex min-h-10 items-center gap-2 rounded-md border bg-muted/50 pr-1 pl-3 text-sm"
							>
								<PaperclipIcon class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
								<span class="min-w-0 flex-1 truncate">{attached.name}</span>
								<Button
									variant="ghost"
									size="icon-sm"
									aria-label={m.ai_chat_file_remove_label({ name: attached.name })}
									onclick={() => (detached = true)}
								>
									<XIcon />
								</Button>
							</div>
							{#if !attached.sendsContent}
								<p class="text-sm text-muted-foreground">{m.ai_chat_file_name_only()}</p>
							{/if}
						{:else}
							<Button
								variant="outline"
								class="justify-start border-dashed"
								onclick={() => (detached = false)}
							>
								<PlusIcon />
								{m.ai_chat_file_attach_button()}
							</Button>
						{/if}
					</div>
				{/if}
				<Field.Field>
					<Field.FieldLabel for="ai-chat-prompt">{m.ai_chat_prompt_label()}</Field.FieldLabel>
					<Textarea
						id="ai-chat-prompt"
						bind:ref={promptInput}
						bind:value={prompt}
						onkeydown={handlePromptKeydown}
						required
					/>
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
					{#if answerAbout !== null}
						<p class="text-sm text-muted-foreground">
							{m.ai_chat_answer_about({ name: answerAbout })}
						</p>
					{/if}
					<p class="text-sm wrap-anywhere whitespace-pre-wrap">{answer}</p>
				{/if}
			</div>
		</div>
	</aside>
{/if}
