<script lang="ts">
	import { nowPlaying } from '$lib/now-playing.svelte';
	import { formatDuration } from '$lib/format';
	import * as m from '$lib/paraglide/messages.js';
	import { Button } from '$lib/components/ui/button';
	import AudioSpectrum from '$lib/components/audio-spectrum.svelte';
	import ListEndIcon from '@lucide/svelte/icons/list-end';
	import PauseIcon from '@lucide/svelte/icons/pause';
	import PlayIcon from '@lucide/svelte/icons/play';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import RotateCwIcon from '@lucide/svelte/icons/rotate-cw';
	import SkipBackIcon from '@lucide/svelte/icons/skip-back';
	import SkipForwardIcon from '@lucide/svelte/icons/skip-forward';
	import Volume2Icon from '@lucide/svelte/icons/volume-2';
	import VolumeXIcon from '@lucide/svelte/icons/volume-x';
	import XIcon from '@lucide/svelte/icons/x';

	let {
		height = $bindable(0)
	}: {
		/** 表示中のバーの高さ (px)。ページの最後が隠れないよう、レイアウトが下の余白に使う。 */
		height?: number;
	} = $props();

	/** 10秒戻す・進むの幅 (秒)。 */
	const SKIP_SECONDS = 10;
	/** 「前の曲」を押したとき、これより進んでいれば曲の頭へ戻す (秒)。 */
	const RESTART_THRESHOLD_SECONDS = 3;

	let audio = $state<HTMLAudioElement>();
	// ADR: 再生位置・長さ・一時停止は bind:currentTime / bind:duration / bind:paused にしない。
	// 曲の差し替えと play() は now-playing が操作の中で行う (→ now-playing.svelte.ts)。
	// バインディングは、差し替えの後に前の曲の位置を書き戻したり、拒まれた play() の例外を
	// 投げ直したりする。状態は要素のイベントで読み、動かすときだけ要素へ書く。
	let currentTime = $state(0);
	let duration = $state(0);

	let progress = $derived(duration > 0 ? (currentTime / duration) * 100 : 0);

	$effect(() => {
		if (audio) return nowPlaying.attach(audio);
	});

	// 曲名がはみ出した分だけ左右に流す (→ docs/ui.md「音声のページ内プレイヤー」)。はみ出していない
	// 距離は0のまま (audio-title-marquee ユーティリティ側で動かない)。
	let titleContainer = $state<HTMLElement>();
	let titleText = $state<HTMLElement>();
	let marqueeDistance = $state(0);

	function measureMarquee() {
		if (!titleContainer || !titleText) {
			marqueeDistance = 0;
			return;
		}
		marqueeDistance = Math.max(0, titleText.scrollWidth - titleContainer.clientWidth);
	}

	$effect(() => {
		// 曲名が変わるたび、幅は変わらなくても測り直す。
		void nowPlaying.current?.title;
		measureMarquee();
	});

	$effect(() => {
		if (!titleContainer || !titleText) return;
		// タブレット⇔デスクトップの段組み変更・ウィンドウ幅の変化に加え、
		// Webフォント読み込み完了などで曲名自体の幅だけが変わる場合も測り直す。
		const observer = new ResizeObserver(measureMarquee);
		observer.observe(titleContainer);
		observer.observe(titleText);
		return () => observer.disconnect();
	});

	// reduced-motionでは曲名の親要素が横スクロール領域になる (→ layout.css の
	// audio-title-clip)。キーボードでも全文へ到達できるよう、そのときだけ
	// フォーカスできるようにする (→ docs/ui.md「UI 全般」の「スクロールする領域は
	// キーボードでも操作できるようにする」と同じ考え方)。
	let prefersReducedMotion = $state(false);

	$effect(() => {
		const query = window.matchMedia('(prefers-reduced-motion: reduce)');
		const update = () => (prefersReducedMotion = query.matches);
		update();
		query.addEventListener('change', update);
		return () => query.removeEventListener('change', update);
	});

	function seek(seconds: number) {
		if (!audio) return;
		const end = duration > 0 ? duration : Number.POSITIVE_INFINITY;
		audio.currentTime = currentTime = Math.min(Math.max(seconds, 0), end);
	}

	function skip(seconds: number) {
		seek(currentTime + seconds);
	}

	function previous() {
		if (currentTime > RESTART_THRESHOLD_SECONDS || !nowPlaying.hasPrevious) {
			seek(0);
		} else {
			nowPlaying.previous();
		}
	}

	function handleEnded() {
		// 曲の終わりで一時停止の状態になっているので、次の曲は明示的に鳴らす。
		if (nowPlaying.autoAdvance) nowPlaying.next({ play: true });
	}

	// ロック画面・イヤホンのボタンからの操作 (→ docs/ui.md「音声のページ内プレイヤー」)。
	$effect(() => {
		if (!('mediaSession' in navigator)) return;
		const session = navigator.mediaSession;
		const track = nowPlaying.current;
		session.metadata = track ? new MediaMetadata({ title: track.title }) : null;
		const handlers: [MediaSessionAction, MediaSessionActionHandler | null][] = [
			['play', () => nowPlaying.resume()],
			['pause', () => nowPlaying.pause()],
			['seekbackward', () => skip(-SKIP_SECONDS)],
			['seekforward', () => skip(SKIP_SECONDS)],
			['previoustrack', () => previous()],
			// 押せない状態では null にして、OS 側のボタンも消す。
			['nexttrack', nowPlaying.hasNext ? () => nowPlaying.next() : null],
			['seekto', (details) => seek(details.seekTime ?? currentTime)]
		];
		for (const [action, handler] of handlers) {
			// 閉じたら OS 側の操作も外す。
			// FIX: 対応していない action を渡すと例外になるブラウザがある。
			try {
				session.setActionHandler(action, track ? handler : null);
			} catch {
				// その操作だけ OS 側から使えない。
			}
		}
	});

	$effect(() => {
		if (!('mediaSession' in navigator)) return;
		if (!nowPlaying.current) {
			navigator.mediaSession.playbackState = 'none';
			return;
		}
		navigator.mediaSession.playbackState = nowPlaying.paused ? 'paused' : 'playing';
		if (duration > 0 && Number.isFinite(duration)) {
			try {
				navigator.mediaSession.setPositionState({
					duration,
					position: Math.min(currentTime, duration)
				});
			} catch {
				// 位置を渡せなくても、操作は効く。
			}
		}
	});
</script>

{#snippet iconButton(
	Icon: typeof XIcon,
	label: string,
	onclick: () => void,
	{
		disabled = false,
		pressed,
		class: className
	}: { disabled?: boolean; pressed?: boolean; class?: string } = {}
)}
	<Button
		variant="ghost"
		size="icon-lg"
		class={['text-sub-foreground', pressed && 'bg-muted text-primary', className]}
		aria-label={label}
		aria-pressed={pressed}
		{disabled}
		{onclick}
	>
		<Icon class="size-5" />
	</Button>
{/snippet}

<!-- 「同時に鳴るのは1トラックのみ」を保証するため常時マウントしておき、
     `nowPlaying.current`の有無だけで表示/非表示を切り替える。 -->
{#if nowPlaying.current}
	{@const track = nowPlaying.current}
	<div
		role="region"
		aria-label={m.audio_player_region_label()}
		bind:offsetHeight={height}
		class="fixed inset-x-0 bottom-0 z-30 flex flex-wrap items-center gap-x-4 border-t bg-background px-4 pt-2 pb-3 shadow-lg md:grid md:audio-player-grid md:items-center md:gap-x-6 md:gap-y-2 md:px-6 md:py-3"
	>
		<!-- スマートフォン幅では、曲名+閉じる / シークバー+時間 / 操作の3段に分ける。
		     タブレット以上では、曲名(左)・操作(中央)・閉じる(右)の1段目と、
		     全幅のシークバーの2段目にする (前へ・再生・次へを中央に置く一般的な音楽アプリの形、→ docs/ui.md「音声のページ内プレイヤー」)。 -->
		<!-- reduced-motion時だけ横スクロール領域になり、キーボードで読み進めるための
		     正当なtabindex (→ docs/ui.md「UI 全般」)。 -->
		<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
		<div
			bind:this={titleContainer}
			class="order-1 min-w-0 flex-1 audio-title-clip md:col-start-1 md:row-start-1"
			role="group"
			tabindex={prefersReducedMotion && marqueeDistance > 0 ? 0 : undefined}
		>
			<p
				bind:this={titleText}
				class={[
					'w-max text-sm font-medium whitespace-nowrap',
					marqueeDistance > 0 && 'audio-title-marquee'
				]}
				style:--marquee-distance="{marqueeDistance}px"
			>
				{track.title}
			</p>
		</div>
		<div
			class="order-2 flex shrink-0 items-center gap-3 self-start md:col-start-3 md:row-start-1 md:self-center md:justify-self-end"
		>
			<!-- ミュート中にも音が出ていると分かるように (→ docs/ui.md「音声のページ内プレイヤー」)。 -->
			{#if nowPlaying.analyser && !prefersReducedMotion}
				<AudioSpectrum analyser={nowPlaying.analyser} paused={nowPlaying.paused} class="h-6 w-16" />
			{/if}
			{@render iconButton(XIcon, m.action_close(), nowPlaying.stop)}
		</div>
		<div
			class="order-4 flex w-full items-center gap-3 text-xs text-muted-foreground md:order-none md:col-span-3 md:row-start-2"
		>
			<span class="shrink-0">{formatDuration(currentTime)}</span>
			<input
				type="range"
				class="audio-seek h-11 min-w-0 flex-1"
				style:--seek-progress="{progress}%"
				min="0"
				max={duration || 0}
				step="any"
				aria-label={m.audio_player_seek_label()}
				aria-valuetext="{formatDuration(currentTime)} / {formatDuration(duration)}"
				bind:value={() => currentTime, seek}
			/>
			<span class="shrink-0">{formatDuration(duration)}</span>
		</div>
		<div
			class="order-3 flex w-full items-center justify-between md:order-none md:col-start-2 md:row-start-1 md:w-auto md:gap-1 md:justify-self-center"
		>
			{@render iconButton(
				ListEndIcon,
				m.audio_player_auto_advance_button(),
				() => (nowPlaying.autoAdvance = !nowPlaying.autoAdvance),
				{ pressed: nowPlaying.autoAdvance }
			)}
			{@render iconButton(SkipBackIcon, m.audio_player_previous_button(), previous)}
			{@render iconButton(RotateCcwIcon, m.audio_player_rewind_button(), () => skip(-SKIP_SECONDS))}
			<Button
				size="icon-lg"
				class="size-12 rounded-full"
				aria-label={nowPlaying.paused
					? m.audio_player_play_button()
					: m.audio_player_pause_button()}
				onclick={nowPlaying.togglePlay}
			>
				{#if nowPlaying.paused}
					<PlayIcon class="size-5 fill-current" />
				{:else}
					<PauseIcon class="size-5 fill-current" />
				{/if}
			</Button>
			{@render iconButton(RotateCwIcon, m.audio_player_forward_button(), () => skip(SKIP_SECONDS))}
			{@render iconButton(SkipForwardIcon, m.audio_player_next_button(), () => nowPlaying.next(), {
				disabled: !nowPlaying.hasNext
			})}
			{@render iconButton(
				nowPlaying.muted ? VolumeXIcon : Volume2Icon,
				m.audio_player_mute_button(),
				() => (nowPlaying.muted = !nowPlaying.muted),
				{ pressed: nowPlaying.muted }
			)}
		</div>
	</div>
{/if}

<!-- 曲をまたいで同じ要素を使い回すため、表示の有無によらず置いておく (→ now-playing.svelte.ts)。 -->
<audio
	bind:this={audio}
	onplay={nowPlaying.syncPaused}
	onpause={nowPlaying.syncPaused}
	onloadstart={() => (currentTime = duration = 0)}
	ontimeupdate={(event) => (currentTime = event.currentTarget.currentTime)}
	ondurationchange={(event) => (duration = event.currentTarget.duration || 0)}
	onended={handleEnded}
></audio>
