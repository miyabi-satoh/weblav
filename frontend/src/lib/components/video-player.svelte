<script lang="ts">
	import Maximize2Icon from '@lucide/svelte/icons/maximize-2';
	import Minimize2Icon from '@lucide/svelte/icons/minimize-2';
	import PauseIcon from '@lucide/svelte/icons/pause';
	import PlayIcon from '@lucide/svelte/icons/play';
	import RepeatIcon from '@lucide/svelte/icons/repeat';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import RotateCwIcon from '@lucide/svelte/icons/rotate-cw';
	import Volume2Icon from '@lucide/svelte/icons/volume-2';
	import VolumeXIcon from '@lucide/svelte/icons/volume-x';
	import { nowPlaying } from '$lib/now-playing.svelte';
	import { videoLoop } from '$lib/video-loop.svelte';
	import { viewerButtonClass } from '$lib/viewer-button';
	import * as m from '$lib/paraglide/messages.js';
	import MediaSeekBar from '$lib/components/media-seek-bar.svelte';
	import { clampTime, SKIP_SECONDS } from '$lib/media';
	import { videoKeyAction } from '$lib/video-keys';

	// ファイルのビューアーの動画 (→ docs/ui.md「PDF・動画・テキストのビューアー」)。
	// 操作は `<video controls>` を使わずに自前で置く。

	let {
		src,
		onerror
	}: {
		src: string;
		onerror: () => void;
	} = $props();

	// Safari の接頭辞付きの全画面。iPadOS 16.4 より前は接頭辞付きしか無く、
	// iPhone は要素の全画面が無く、動画だけを OS の全画面にできる。
	type WebkitDocument = Document & {
		webkitFullscreenEnabled?: boolean;
		webkitFullscreenElement?: Element | null;
		webkitExitFullscreen?: () => void;
	};
	type WebkitElement = HTMLElement & { webkitRequestFullscreen?: () => void };
	type WebkitVideo = HTMLVideoElement & { webkitEnterFullscreen?: () => void };

	let container = $state<WebkitElement>();
	let video = $state<WebkitVideo>();
	// ADR: 音声のプレイヤーと同じく、状態は要素のイベントで読み、動かすときだけ要素へ書く
	// (→ audio-player-bar.svelte)。
	let paused = $state(true);
	let muted = $state(false);
	let currentTime = $state(0);
	let duration = $state(0);
	let fullscreen = $state(false);

	// ビューアーは開いてから描くので、document は常にある。
	const doc = document as WebkitDocument;
	let canFullscreen = $derived(
		doc.fullscreenEnabled ||
			doc.webkitFullscreenEnabled === true ||
			typeof video?.webkitEnterFullscreen === 'function'
	);

	$effect(() => {
		const update = () =>
			(fullscreen = (doc.fullscreenElement ?? doc.webkitFullscreenElement ?? null) === container);
		document.addEventListener('fullscreenchange', update);
		document.addEventListener('webkitfullscreenchange', update);
		return () => {
			document.removeEventListener('fullscreenchange', update);
			document.removeEventListener('webkitfullscreenchange', update);
		};
	});

	function togglePlay() {
		if (!video) return;
		if (video.paused) {
			// 読み込めなければ error のイベントで案内に替わるので、ここでは拒否を捨てる。
			video.play().catch(() => {});
		} else {
			video.pause();
		}
	}

	function seek(seconds: number) {
		if (!video) return;
		video.currentTime = currentTime = clampTime(seconds, duration);
	}

	function handleKeydown(event: KeyboardEvent) {
		// シークバーは、自身の左右キーで位置を動かす。
		if (event.target instanceof HTMLInputElement) return;
		// 標準の操作部品と同じく、動画の中の左右キーは再生位置に使い、ビューアーの前後の移動に回さない。
		// 全画面で見ている間に、別のファイルへ替わって全画面が解けないようにするため。
		if (event.key === 'ArrowLeft') seek(currentTime - SKIP_SECONDS);
		else if (event.key === 'ArrowRight') seek(currentTime + SKIP_SECONDS);
		else return;
		event.preventDefault();
		event.stopPropagation();
	}

	// 左右キーのほかは、ビューアーのどこにフォーカスがあっても効かせる (動画サイトと同じキー → $lib/video-keys.ts)。
	// 開いた直後のフォーカスは動画の外にあり、動画の中だけで受けると押しても効かないため。
	function handleDocumentKeydown(event: KeyboardEvent) {
		const action = videoKeyAction(event);
		if (!action) return;
		if (action.type === 'toggle-play') togglePlay();
		else if (action.type === 'rewind') seek(currentTime - SKIP_SECONDS);
		else if (action.type === 'forward') seek(currentTime + SKIP_SECONDS);
		else if (action.type === 'toggle-mute') muted = !muted;
		else if (action.type === 'toggle-fullscreen') {
			if (!canFullscreen) return;
			// フォーカスが全画面にする要素の外 (「新しいタブで開く」など) にあると、body に落ちて左右キーがどこにも届かなくなる。
			// ボタンで入ったときと同じく、動画の中に置く。
			if (!fullscreen) video?.focus({ preventScroll: true });
			toggleFullscreen();
		} else if (duration > 0) seek(duration * action.fraction);
		else return;
		event.preventDefault();
	}

	function toggleFullscreen() {
		if (!container || !video) return;
		if (fullscreen) {
			if (doc.fullscreenElement) void doc.exitFullscreen();
			else doc.webkitExitFullscreen?.();
		} else if (doc.fullscreenEnabled) {
			void container.requestFullscreen();
		} else if (doc.webkitFullscreenEnabled) {
			container.webkitRequestFullscreen?.();
		} else {
			// iPhone。OS の全画面では OS の操作部品に替わる。
			video.webkitEnterFullscreen?.();
		}
	}
</script>

<svelte:document onkeydown={handleDocumentKeydown} />

{#snippet iconButton(Icon: typeof PlayIcon, label: string, onclick: () => void, pressed?: boolean)}
	<button
		type="button"
		class={[viewerButtonClass, pressed && 'bg-white/20 text-white']}
		aria-label={label}
		aria-pressed={pressed}
		{onclick}
	>
		<Icon class="size-5" />
	</button>
{/snippet}

<!-- 中のボタンから上がってきた左右キーを受けるだけで、この要素自体はフォーカスを受けない。 -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
	bind:this={container}
	class="flex size-full flex-col bg-black"
	role="region"
	aria-label={m.video_player_region_label()}
	onkeydown={handleKeydown}
>
	<div class="relative min-h-0 flex-1">
		<!-- 押すと再生・一時停止 (動画サイトのプレイヤーと同じ)。キーボードでは下の再生ボタンを使う。
		     押したときにフォーカスを取り、続く左右キーを再生位置に回す (→ handleKeydown)。
		     Tab の巡回には入れない。キーボードでは下の操作に入れば左右キーが効くため。
		     字幕は、置かれた動画のファイルしか無いので付けられない。 -->
		<!-- svelte-ignore a11y_media_has_caption -->
		<video
			bind:this={video}
			{src}
			class="size-full object-contain outline-none"
			tabindex="-1"
			playsinline
			loop={videoLoop.value}
			bind:muted
			onclick={togglePlay}
			onplay={() => {
				paused = false;
				// 音声のプレイヤーと同時に鳴らさない。
				nowPlaying.pause();
			}}
			onpause={() => (paused = true)}
			ontimeupdate={(event) => (currentTime = event.currentTarget.currentTime)}
			ondurationchange={(event) => (duration = event.currentTarget.duration || 0)}
			{onerror}
		></video>
		{#if paused}
			<!-- 標準の操作部品が無いので、止まっている間は中央に再生ボタンを出して、押せば鳴ると分かるようにする。 -->
			<button
				type="button"
				class="absolute top-1/2 left-1/2 inline-flex size-16 -translate-1/2 items-center justify-center rounded-full bg-black/60 text-white transition-colors hover:bg-black/80"
				aria-label={m.media_play_button()}
				onclick={togglePlay}
			>
				<PlayIcon class="size-7 fill-current" />
			</button>
		{/if}
	</div>

	<div
		class="flex shrink-0 flex-wrap items-center gap-x-1 px-1 pb-1 md:flex-nowrap md:gap-x-2 md:px-2"
	>
		<div
			class="flex w-full items-center gap-3 px-2 text-xs text-white/80 md:order-2 md:w-auto md:flex-1"
		>
			<MediaSeekBar {currentTime} {duration} onseek={seek} onDark />
		</div>
		<div class="flex items-center md:order-1">
			{@render iconButton(
				paused ? PlayIcon : PauseIcon,
				paused ? m.media_play_button() : m.media_pause_button(),
				togglePlay
			)}
			{@render iconButton(RotateCcwIcon, m.media_rewind_button(), () =>
				seek(currentTime - SKIP_SECONDS)
			)}
			{@render iconButton(RotateCwIcon, m.media_forward_button(), () =>
				seek(currentTime + SKIP_SECONDS)
			)}
		</div>
		<div class="ml-auto flex items-center md:order-3 md:ml-0">
			{@render iconButton(
				RepeatIcon,
				m.video_player_loop_button(),
				() => (videoLoop.value = !videoLoop.value),
				videoLoop.value
			)}
			{@render iconButton(
				muted ? VolumeXIcon : Volume2Icon,
				m.media_mute_button(),
				() => (muted = !muted),
				muted
			)}
			{#if canFullscreen}
				{@render iconButton(
					fullscreen ? Minimize2Icon : Maximize2Icon,
					fullscreen ? m.video_player_exit_fullscreen_button() : m.video_player_fullscreen_button(),
					toggleFullscreen
				)}
			{/if}
		</div>
	</div>
</div>
