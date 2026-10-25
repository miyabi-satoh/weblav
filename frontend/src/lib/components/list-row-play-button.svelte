<script lang="ts">
	import type { Snippet } from 'svelte';
	import { browseLayout } from '$lib/browse-layout.svelte';
	import { browseRowClass, browseRowPressClass, browseRowTextClass } from '$lib/list-row';
	import { cn } from '$lib/utils';
	import { nowPlaying, type Track, type TrackSource } from '$lib/now-playing.svelte';
	import ListRowIcon from '$lib/components/list-row-icon.svelte';
	import * as m from '$lib/paraglide/messages.js';

	// 閲覧側の一覧の、ページ内プレイヤーで再生する音声の行・タイル (→ $lib/list-row.ts)。
	let {
		track,
		queue,
		more,
		compact = false,
		children,
		subtitle,
		leading,
		trailing
	}: {
		/** キューと同じ組み立て方で作る (押した行をキューの中から `src` で探すため)。 */
		track: Track;
		/**
		 * この行が並ぶ一覧の音声 (表示順)。プレイヤーの前・次の曲になる。
		 * 関数なら押したときに呼ぶ (区切って取る一覧は、手元にある曲から押した時点で作る)。
		 */
		queue: Track[] | (() => Track[]);
		/** `queue` の続きを足す口 (→ `nowPlaying.play`)。 */
		more?: TrackSource;
		/** フォルダー一覧の詰めた行 (`browseRowClass(true)`) にする。タイルには効かない。 */
		compact?: boolean;
		/** 行のタイトル。これを包むボタンの押せる範囲が、行いっぱいに広がる。 */
		children: Snippet;
		/** タイトルの下に置く2段目。 */
		subtitle?: Snippet;
		/** 行の文字より前に置くもの (管理画面の選択のチェックなど)。行いっぱいのリンクの上に重ねて置く。 */
		leading?: Snippet;
		/** 行の文字と「再生中」の印の間に置くもの。 */
		trailing?: Snippet;
	} = $props();

	let current: 'playing' | 'paused' | undefined = $derived(
		nowPlaying.current?.src === track.src ? (nowPlaying.paused ? 'paused' : 'playing') : undefined
	);
</script>

<div
	class={cn(
		browseRowClass(compact),
		// 同じタイトルが並ぶことがあるので、どれが鳴っているかを行の塗りと左の線 (タイルは枠) でも示す。
		current && 'bg-primary/10 hover:bg-primary/15',
		current &&
			(browseLayout.tile
				? 'border-primary'
				: 'before:absolute before:inset-y-0 before:left-0 before:w-0.75 before:bg-primary')
	)}
>
	{@render leading?.()}
	<ListRowIcon play {current} {compact} />
	<span class={browseRowTextClass()}>
		<!-- ボタンは文字が中央に寄るので、行の寄せ (左) に合わせる。 -->
		<button
			type="button"
			class={[browseRowPressClass(), 'text-left']}
			// 鳴っている行は一時停止のアイコンを出すので、押すと止める (下のバーのボタンと同じ動き)。
			onclick={() =>
				current === 'playing'
					? nowPlaying.pause()
					: nowPlaying.play(track, typeof queue === 'function' ? queue() : queue, more)}
		>
			{@render children()}
			{#if current}
				<span class="sr-only">{m.now_playing_label()}</span>
			{/if}
		</button>
		{@render subtitle?.()}
	</span>
	{@render trailing?.()}
	<!-- 読み上げにはボタンの名前で渡す。タイルは幅が狭いので、文字では出さない (→ docs/ui.md「UI 全般」)。 -->
	{#if current && !browseLayout.tile}
		<span class="shrink-0 text-xs text-primary" aria-hidden="true">{m.now_playing_label()}</span>
	{/if}
</div>
