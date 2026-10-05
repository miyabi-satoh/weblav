<script lang="ts">
	import PlayIcon from '@lucide/svelte/icons/play';
	import PauseIcon from '@lucide/svelte/icons/pause';
	import type FileIcon from '@lucide/svelte/icons/file';
	import { browseLayout } from '$lib/browse-layout.svelte';
	import { THUMBNAIL_FOR_ATTRIBUTE } from '$lib/image-viewer';

	// 1列リストの行の先頭、タイルの上に置くアイコン枠 (→ $lib/list-row.ts、タイルの大きさは docs/ui.md「UI 全般」)。
	// `play` はページ内で再生する音声の行で、差し色の丸に再生アイコンを入れる。
	// `current` はプレイヤーに載っている曲の行で、丸を塗り、鳴っている間は一時停止のアイコンにする (下のバーと揃える)。
	// `thumbnail` は縮小画像のある行 (画像と、OS が縮小画像を作れた PDF・動画・Office などのファイル) で、
	// 縮小画像を枠いっぱいに出す。読めなければ `icon` に戻す。
	let {
		icon: Icon,
		play = false,
		current,
		accent = false,
		compact = false,
		thumbnail
	}: {
		icon?: typeof FileIcon;
		play?: boolean;
		/** プレイヤーに載っている曲の行なら、鳴っているか (`playing`) 止まっているか (`paused`)。 */
		current?: 'playing' | 'paused';
		accent?: boolean;
		/** フォルダ一覧の詰めた行 (`browseRowClass(true)`) と組み合わせる。タイルには効かない。 */
		compact?: boolean;
		/**
		 * `src` は縮小画像、`original` は元のファイルの URL。画像のビューアは `original` で行の縮小画像を探し、
		 * 開閉のアニメーションの起点にする (→ $lib/image-viewer.ts)。
		 */
		thumbnail?: { src: string; original: string };
	} = $props();

	// 失敗した URL を覚える。行が使い回されて別の画像になったら、改めて読みに行く。
	let failedSrc = $state<string | null>(null);
	let showThumbnail = $derived(thumbnail !== undefined && thumbnail.src !== failedSrc);

	let tile = $derived(browseLayout.tile);
	let frameSizeClass = $derived(tile ? 'size-18' : compact ? 'size-9 sm:size-11' : 'size-11');
	let playIconSizeClass = $derived(tile ? 'size-7' : compact ? 'size-3.5 sm:size-4' : 'size-4');
</script>

<span
	class={[
		'flex shrink-0 items-center justify-center',
		frameSizeClass,
		play && 'rounded-full border-2 border-primary',
		play && current && 'bg-primary'
	]}
>
	{#if play && current === 'playing'}
		<PauseIcon class="fill-primary-foreground text-primary-foreground {playIconSizeClass}" />
	{:else if play}
		<PlayIcon
			class={[
				current ? 'fill-primary-foreground text-primary-foreground' : 'fill-primary text-primary',
				playIconSizeClass
			]}
		/>
	{:else if thumbnail && showThumbnail}
		<!-- 行の文字が何の画像かを伝えるので、読み上げでは飛ばす。 -->
		<img
			src={thumbnail.src}
			alt=""
			loading="lazy"
			decoding="async"
			class={['size-full bg-muted object-cover', tile ? 'rounded-md' : 'rounded-sm']}
			onerror={() => (failedSrc = thumbnail.src)}
			{...{ [THUMBNAIL_FOR_ATTRIBUTE]: thumbnail.original }}
		/>
	{:else if Icon}
		<Icon
			class={[tile ? 'size-10' : 'size-6', accent ? 'text-primary' : 'text-sub-foreground']}
			strokeWidth={1.5}
		/>
	{/if}
</span>
