<script lang="ts">
	import { formatDuration } from '$lib/format';
	import { seekDrag } from '$lib/seek-drag';
	import * as m from '$lib/paraglide/messages.js';

	// 音声のプレイヤーと動画のビューアーのシークバー。今の位置・シークバー・長さを横に並べる。
	// 並べる行 (幅・文字色) は呼び出し側が用意する。

	let {
		currentTime,
		duration,
		onseek,
		onDark = false
	}: {
		currentTime: number;
		duration: number;
		onseek: (seconds: number) => void;
		/** 黒い地 (動画のビューアー) に置く。 */
		onDark?: boolean;
	} = $props();

	let progress = $derived(duration > 0 ? (currentTime / duration) * 100 : 0);
</script>

<span class="shrink-0">{formatDuration(currentTime)}</span>
<!-- FIX: WebKit は、値を書き込んでいない range の max が変わると、値を min と max の中ほどに置き直す。
     長さが分かったときに一度だけ作り直して、今の位置を書き込ませる (一度書けば、その後 max が変わっても動かない)。 -->
{#key duration > 0}
	<input
		type="range"
		class="media-seek h-11 min-w-0 flex-1 touch-none"
		style:--seek-progress="{progress}%"
		style:--seek-fill={onDark ? 'white' : undefined}
		style:--seek-track={onDark ? 'rgb(255 255 255 / 0.3)' : undefined}
		min="0"
		max={duration || 0}
		step="any"
		aria-label={m.media_seek_label()}
		aria-valuetext="{formatDuration(currentTime)} / {formatDuration(duration)}"
		bind:value={() => currentTime, onseek}
		{@attach seekDrag(onseek)}
	/>
{/key}
<span class="shrink-0">{formatDuration(duration)}</span>
