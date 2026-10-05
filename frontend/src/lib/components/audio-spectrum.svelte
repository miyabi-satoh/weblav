<script lang="ts">
	import { spectrumBars } from '$lib/spectrum';

	let {
		analyser,
		paused,
		class: className
	}: {
		analyser: AnalyserNode;
		/** 止めている間は、棒が下りきったら描くのをやめる。 */
		paused: boolean;
		class?: string;
	} = $props();

	/** 棒の数。行の高さに収まる小ささでも、低い音から高い音への動きが見分けられる数。 */
	const BAR_COUNT = 12;
	/** 棒の間のすき間 (CSS px)。 */
	const BAR_GAP = 2;

	let canvas = $state<HTMLCanvasElement>();
	let running = $state(false);

	$effect(() => {
		const context = analyser.context;
		const update = () => (running = context.state === 'running');
		update();
		context.addEventListener('statechange', update);
		return () => context.removeEventListener('statechange', update);
	});

	$effect(() => {
		if (!canvas) return;
		const target = canvas;
		const context = target.getContext('2d');
		if (!context) return;
		const data = new Uint8Array(analyser.frequencyBinCount);
		// 音の処理が止まっている (iPad で画面を消した後など) と前の値が返り続けるので、棒を下ろして止め、
		// 再開したら描き直す。
		const stopWhenSilent = paused || !running;
		let frame = 0;

		const draw = () => {
			const ratio = window.devicePixelRatio || 1;
			const width = target.clientWidth;
			const height = target.clientHeight;
			if (target.width !== Math.round(width * ratio)) target.width = Math.round(width * ratio);
			if (target.height !== Math.round(height * ratio)) target.height = Math.round(height * ratio);
			context.setTransform(ratio, 0, 0, ratio, 0, 0);
			context.clearRect(0, 0, width, height);

			if (running) analyser.getByteFrequencyData(data);
			else data.fill(0);
			const bars = spectrumBars(data, analyser.context.sampleRate, BAR_COUNT);
			const barWidth = (width - BAR_GAP * (BAR_COUNT - 1)) / BAR_COUNT;
			// 色は CSS の text-primary から取る。ライト・ダークの切り替えにも付いていく。
			context.fillStyle = getComputedStyle(target).color;
			bars.forEach((bar, i) => {
				// 無音でも1px の線を残し、棒の置き場がそこにあると分かるようにする。
				const barHeight = Math.max(1, bar * height);
				context.fillRect(i * (barWidth + BAR_GAP), height - barHeight, barWidth, barHeight);
			});

			if (stopWhenSilent && bars.every((bar) => bar === 0)) return;
			frame = requestAnimationFrame(draw);
		};
		draw();
		return () => cancelAnimationFrame(frame);
	});
</script>

<!-- 飾りなので読み上げない。 -->
<canvas bind:this={canvas} class={['text-primary', className]} aria-hidden="true"></canvas>
