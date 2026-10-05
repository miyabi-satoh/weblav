<script lang="ts">
	import type { Snippet } from 'svelte';
	import { browseLayout } from '$lib/browse-layout.svelte';
	import { browseRowClass } from '$lib/list-row';
	import { cn } from '$lib/utils';
	import { nowPlaying, type Track } from '$lib/now-playing.svelte';
	import ListRowIcon from '$lib/components/list-row-icon.svelte';
	import * as m from '$lib/paraglide/messages.js';

	// 閲覧側の一覧の、ページ内プレイヤーで再生する音声の行・タイル (→ $lib/list-row.ts)。
	let {
		track,
		queue,
		compact = false,
		children
	}: {
		/** キューと同じ組み立て方で作る (押した行をキューの中から `src` で探すため)。 */
		track: Track;
		/** この行が並ぶ一覧の音声 (表示順)。プレイヤーの前・次の曲になる。 */
		queue: Track[];
		/** フォルダ一覧の詰めた行 (`browseRowClass(true)`) にする。タイルには効かない。 */
		compact?: boolean;
		/** アイコンと「再生中」の印の間に置く、行の文字。 */
		children: Snippet;
	} = $props();

	let current: 'playing' | 'paused' | undefined = $derived(
		nowPlaying.current?.src === track.src ? (nowPlaying.paused ? 'paused' : 'playing') : undefined
	);
</script>

<button
	type="button"
	class={cn(
		browseRowClass(compact),
		// 同じタイトルが並ぶことがあるので、どれが鳴っているかを行の塗りと左の線 (タイルは枠) でも示す。
		current && 'bg-primary/10 hover:bg-primary/15',
		current &&
			(browseLayout.tile
				? 'border-primary'
				: 'relative before:absolute before:inset-y-0 before:left-0 before:w-0.75 before:bg-primary')
	)}
	// 鳴っている行は一時停止のアイコンを出すので、押すと止める (下のバーのボタンと同じ動き)。
	onclick={() => (current === 'playing' ? nowPlaying.pause() : nowPlaying.play(track, queue))}
>
	<ListRowIcon play {current} {compact} />
	{@render children()}
	{#if current}
		<!-- タイルでは読み上げにだけ渡す (→ docs/ui.md「UI 全般」)。 -->
		<span class={browseLayout.tile ? 'sr-only' : 'shrink-0 text-xs text-primary'}>
			{m.now_playing_label()}
		</span>
	{/if}
</button>
