<script lang="ts">
	/**
	 * ファイル選択のドロップゾーン。`<input type=file>` を透明化して `label` 全体に
	 * 重ね、クリック・キーボードでの選択・必須バリデーションは input の標準動作に
	 * 任せる (visibility:hidden/sr-only にすると required のバブルが input の実位置
	 * に出て見た目とずれるため、透明のまま同じ大きさで重ねている)。
	 * ドラッグ&ドロップだけを label に足す。
	 */
	import { cn } from '$lib/utils';
	import UploadIcon from '@lucide/svelte/icons/upload';

	type Props = {
		id: string;
		files?: FileList;
		required?: boolean;
		hint: string;
		class?: string;
	};

	let { id, files = $bindable(), required = false, hint, class: className }: Props = $props();

	let inputRef = $state<HTMLInputElement | null>(null);
	// dragenter/dragleaveは子要素への出入りでも発火するため、ネスト分を数えて
	// 0に戻ったときだけドラッグ中の表示を解く。
	let dragDepth = 0;
	let dragging = $state(false);

	function handleDragEnter(event: DragEvent) {
		event.preventDefault();
		dragDepth++;
		dragging = true;
	}

	function handleDragOver(event: DragEvent) {
		event.preventDefault();
	}

	function handleDragLeave(event: DragEvent) {
		event.preventDefault();
		dragDepth = Math.max(0, dragDepth - 1);
		if (dragDepth === 0) dragging = false;
	}

	function handleDrop(event: DragEvent) {
		event.preventDefault();
		dragDepth = 0;
		dragging = false;
		const dropped = event.dataTransfer?.files;
		if (dropped && dropped.length > 0 && inputRef) {
			// input.files への代入で、input自体もbind:filesと同じ状態になる
			// (フォーム送信・再選択の見た目を input 側の値と一致させるため)。
			inputRef.files = dropped;
			files = dropped;
		}
	}

	// Escでドラッグをキャンセルされると dragleave/drop が発火せず dragDepth が
	// 戻らないため、ドラッグ中だけキーで明示的にリセットする。
	$effect(() => {
		if (!dragging) return;
		function handleKeydown(event: KeyboardEvent) {
			if (event.key !== 'Escape') return;
			dragDepth = 0;
			dragging = false;
		}
		window.addEventListener('keydown', handleKeydown);
		return () => window.removeEventListener('keydown', handleKeydown);
	});

	let fileName = $derived(files?.[0]?.name ?? null);
</script>

<label
	for={id}
	class={cn(
		'relative flex h-24 w-full cursor-pointer flex-col items-center justify-center gap-1 rounded-lg border border-dashed border-input bg-transparent px-4 text-center text-sm text-muted-foreground transition-colors hover:bg-accent/50',
		'has-focus-visible:border-ring has-focus-visible:ring-3 has-focus-visible:ring-ring/50',
		dragging && 'border-ring bg-accent/50 text-foreground',
		className
	)}
	ondragenter={handleDragEnter}
	ondragover={handleDragOver}
	ondragleave={handleDragLeave}
	ondrop={handleDrop}
>
	<UploadIcon class="size-5" aria-hidden="true" />
	{#if fileName}
		<span class="max-w-full truncate font-medium text-foreground">{fileName}</span>
	{:else}
		<span>{hint}</span>
	{/if}
	<input
		bind:this={inputRef}
		{id}
		type="file"
		class="absolute inset-0 h-full w-full cursor-pointer opacity-0"
		bind:files
		{required}
	/>
</label>
