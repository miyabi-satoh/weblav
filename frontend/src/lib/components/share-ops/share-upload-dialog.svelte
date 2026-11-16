<script lang="ts" module>
	import type { components } from '#lib/api/schema.js';
	import { formatByteSize } from '#lib/format.ts';
	import * as m from '#lib/paraglide/messages.js';

	type UploadedFile = components['schemas']['UploadedFile'];

	/** 選んだ1ファイルの様子。 */
	export type UploadItem = {
		file: File;
		state: 'waiting' | 'uploading' | 'done' | 'failed' | 'tooLarge' | 'folder';
		/** 送ったバイトの割合 (0〜1)。 */
		progress: number;
		/** 置いた結果。名前を変えて置いたときに、その名前を出す。 */
		result?: UploadedFile;
		/** 失敗の理由。 */
		message?: string;
	};

	/** 行の2段目に出す文言。 */
	export function uploadItemNote(item: UploadItem, maxBytes: number | null): string | undefined {
		if (item.state === 'tooLarge' && maxBytes !== null) {
			return m.shares_upload_too_large({ size: formatByteSize(maxBytes) });
		}
		if (item.state === 'folder') return m.shares_upload_folder();
		if (item.state === 'failed') return item.message;
		if (item.state === 'done' && item.result) {
			if (item.result.extensionFixed) {
				return m.shares_upload_extension_fixed({ name: item.result.name });
			}
			if (item.result.renamed) return m.shares_upload_renamed({ name: item.result.name });
		}
		return undefined;
	}
</script>

<script lang="ts">
	/**
	 * 共有の中のフォルダーへのアップロード (→ docs/ui.md「共有の中の画面」)。
	 * いくつも選べ、1ファイルずつ送って、ファイルごとに進み具合を出す。同じ名前があればサーバーが番号を付けて置き、
	 * 画像の拡張子が中身と合わなければ直すので、そうしたものは置いた名前を添える。
	 * 上限を超えるファイルとドロップされたフォルダーは送らずに印を付ける (送ってから断られると、待った分が無駄になるため)。
	 * 送っている間は、画面を移ろうとしたら確かめる。移った先からは送りかけが見えず、止めようもないため。
	 */
	import { onDestroy, untrack } from 'svelte';
	import { beforeNavigate, goto } from '$app/navigation';
	import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import FileIcon from '@lucide/svelte/icons/file';
	import { shareUploadUrl, uploadShareFile } from '#lib/share-upload.ts';
	import { Button } from '#lib/components/ui/button/index.ts';
	import * as Dialog from '#lib/components/ui/dialog/index.ts';
	import { Progress } from '#lib/components/ui/progress/index.ts';
	import * as AlertDialog from '#lib/components/ui/alert-dialog/index.ts';
	import FileDropzone, { isDroppedFolder } from '#lib/components/file-dropzone.svelte';
	import LoadingButton from '#lib/components/loading-button.svelte';
	import { listItemClass } from '#lib/list-row.ts';

	let {
		open = $bindable(),
		shareId,
		path,
		folderName,
		maxBytes,
		ondone
	}: {
		open: boolean;
		shareId: number;
		/** 置き先のフォルダーの、共有からの相対パス。 */
		path: string;
		/** 置き先のフォルダーの名前 (見出しに出す)。 */
		folderName: string;
		/** 1ファイルの上限 (バイト)。無ければ `null`。 */
		maxBytes: number | null;
		/** 1つでも置いたら、送り終えた後に画面を取り直す。 */
		ondone: () => void | Promise<void>;
	} = $props();

	let picked = $state<FileList | undefined>(undefined);
	let items = $state<UploadItem[]>([]);
	let uploading = $state(false);
	let controller: AbortController | null = null;
	/** 送っている途中の `start()`。止めた後、送り終えたことにしてから移るために待つ。 */
	let running: Promise<void> | null = null;
	/** 送っている間に移ろうとした行き先。確かめを出している間だけ `null` でない。 */
	let leaveTo = $state<URL | null>(null);

	let waiting = $derived(items.filter((item) => item.state === 'waiting').length);
	// 送ったものがあり、送るものが残っていない。上限を超えて送らなかったものだけなら、選び直せるよう終わりにしない。
	let finished = $derived(
		items.some((item) => item.state === 'done' || item.state === 'failed') &&
			items.every((item) => item.state !== 'waiting' && item.state !== 'uploading')
	);

	// 開くたびに空から始める。
	$effect.pre(() => {
		if (!open) return;
		untrack(() => {
			picked = undefined;
			items = [];
		});
	});

	// 選び直したら、まだ送っていない並びを入れ替える。送り終えたものの結果は、選び直した時点で消す。
	$effect(() => {
		const list = picked;
		if (!list || list.length === 0) return;
		untrack(() => {
			if (uploading) return;
			items = Array.from(list, (file) => ({
				file,
				state: isDroppedFolder(file)
					? 'folder'
					: maxBytes !== null && file.size > maxBytes
						? 'tooLarge'
						: 'waiting',
				progress: 0
			}));
		});
	});

	function start() {
		if (uploading || waiting === 0) return;
		running = upload().finally(() => (running = null));
	}

	async function upload() {
		uploading = true;
		controller = new AbortController();
		const url = shareUploadUrl(shareId, path);
		let placed = false;
		for (const item of items) {
			if (item.state !== 'waiting') continue;
			if (controller.signal.aborted) break;
			item.state = 'uploading';
			const outcome = await uploadShareFile(
				url,
				item.file,
				(ratio) => (item.progress = ratio),
				controller.signal
			);
			if (outcome.ok) {
				item.state = 'done';
				item.progress = 1;
				item.result = outcome.file;
				placed = true;
			} else if (outcome.aborted) {
				// 止めたものは、もう一度送れるよう待ちに戻す。
				item.state = 'waiting';
				item.progress = 0;
			} else {
				item.state = 'failed';
				item.message = outcome.message;
			}
		}
		controller = null;
		uploading = false;
		if (placed) await ondone();
	}

	function stop() {
		controller?.abort();
	}

	beforeNavigate((navigation) => {
		if (!uploading) return;
		// タブを閉じる・再読み込みは、ブラウザー標準の確認に任せる。
		if (navigation.type === 'leave') {
			navigation.cancel();
			return;
		}
		if (!navigation.to) return;
		navigation.cancel();
		leaveTo = navigation.to.url;
	});

	/** 送るのを止めて、止めた移動を続ける。送り終えてから移る (送っている間の移動は、もう一度止まるため)。 */
	async function stopAndLeave() {
		const target = leaveTo;
		leaveTo = null;
		if (target === null) return;
		stop();
		await running;
		open = false;
		// 行き先は SvelteKit のナビゲーションが解決済みの URL。
		// eslint-disable-next-line svelte/no-navigation-without-resolve
		await goto(target);
	}

	// 移る以外で窓ごと無くなっても、見えないまま送り続けない。
	onDestroy(stop);

	// 窓を開いている間は、ドロップの場所の外に落としたファイルをブラウザーに開かせない。開くと画面ごと移り、送っていれば残りが止まる。
	// 送っている間はドロップの場所が無いので、窓のどこに落としても受けない。ドロップの場所 (`FileDropzone`) が受けたものは、
	// そちらが先に止めているので触らない。
	$effect(() => {
		if (!open) return;
		function refuse(event: DragEvent) {
			if (event.defaultPrevented) return;
			event.preventDefault();
			if (event.dataTransfer) event.dataTransfer.dropEffect = 'none';
		}
		window.addEventListener('dragover', refuse);
		window.addEventListener('drop', refuse);
		return () => {
			window.removeEventListener('dragover', refuse);
			window.removeEventListener('drop', refuse);
		};
	});
</script>

<Dialog.Root bind:open>
	<!-- 送っている間は閉じない。閉じると送りかけが見えないまま止まるため。 -->
	<Dialog.Content
		class="sm:max-w-lg"
		showCloseButton={!uploading}
		interactOutsideBehavior={uploading || waiting > 0 ? 'ignore' : 'close'}
		escapeKeydownBehavior={uploading ? 'ignore' : 'close'}
	>
		<Dialog.Header>
			<Dialog.Title>{m.shares_upload_title({ folder: folderName })}</Dialog.Title>
		</Dialog.Header>
		<Dialog.Body class="flex flex-col gap-4">
			{#if !uploading && !finished}
				<FileDropzone
					id="share-upload-files"
					multiple
					bind:files={picked}
					hint={m.shares_upload_hint()}
				/>
			{/if}
			{#if items.length > 0}
				<ul class="flex flex-col" aria-label={m.shares_upload_list_label()}>
					{#each items as item, index (index)}
						{@const note = uploadItemNote(item, maxBytes)}
						<li class={[listItemClass, 'flex items-start gap-3 py-2']}>
							{#if item.state === 'done'}
								<CircleCheckIcon class="mt-0.5 size-4 shrink-0 text-primary" />
							{:else if item.state === 'failed' || item.state === 'tooLarge' || item.state === 'folder'}
								<CircleAlertIcon class="mt-0.5 size-4 shrink-0 text-destructive" />
							{:else}
								<FileIcon class="mt-0.5 size-4 shrink-0 text-muted-foreground" />
							{/if}
							<div class="flex min-w-0 flex-1 flex-col gap-1">
								<span class="text-sm wrap-anywhere">{item.file.name}</span>
								{#if item.state === 'uploading'}
									<Progress
										class="h-1.5"
										aria-label={item.file.name}
										value={Math.round(item.progress * 100)}
									/>
								{/if}
								{#if note}
									<span
										class={[
											'text-xs wrap-anywhere',
											item.state === 'done' ? 'text-muted-foreground' : 'text-destructive'
										]}>{note}</span
									>
								{/if}
							</div>
							<span class="shrink-0 text-xs text-muted-foreground"
								>{formatByteSize(item.file.size)}</span
							>
						</li>
					{/each}
				</ul>
			{/if}
		</Dialog.Body>
		<Dialog.Footer>
			{#if uploading}
				<Button type="button" variant="outline" onclick={stop}>
					{m.action_stop()}
				</Button>
				<LoadingButton type="button" loading>{m.shares_upload_submit()}</LoadingButton>
			{:else if finished}
				<Button type="button" onclick={() => (open = false)}>{m.action_close()}</Button>
			{:else}
				<Button type="button" variant="outline" onclick={() => (open = false)}>
					{m.action_cancel()}
				</Button>
				<LoadingButton type="button" disabled={waiting === 0} onclick={start}>
					{m.shares_upload_submit()}
				</LoadingButton>
			{/if}
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>

<AlertDialog.Root
	bind:open={
		() => leaveTo !== null,
		(value) => {
			if (!value) leaveTo = null;
		}
	}
>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{m.shares_upload_leave_title()}</AlertDialog.Title>
			<AlertDialog.Description>{m.shares_upload_leave_description()}</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>{m.action_cancel()}</AlertDialog.Cancel>
			<AlertDialog.Action variant="destructive" onclick={stopAndLeave}
				>{m.shares_upload_leave_button()}</AlertDialog.Action
			>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
