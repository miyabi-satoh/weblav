import { afterEach, describe, expect, it } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import AudioPlayerBar from './audio-player-bar.svelte';
import { nowPlaying } from '$lib/now-playing.svelte';

/** 無音の WAV (8kHz・8bit・モノラル) を、指定の秒数で作る。 */
function silentWavUrl(seconds: number): string {
	const samples = 8000 * seconds;
	const buffer = new ArrayBuffer(44 + samples);
	const view = new DataView(buffer);
	const text = (offset: number, value: string) =>
		[...value].forEach((char, i) => view.setUint8(offset + i, char.charCodeAt(0)));
	text(0, 'RIFF');
	view.setUint32(4, 36 + samples, true);
	text(8, 'WAVEfmt ');
	view.setUint32(16, 16, true);
	view.setUint16(20, 1, true);
	view.setUint16(22, 1, true);
	view.setUint32(24, 8000, true);
	view.setUint32(28, 8000, true);
	view.setUint16(32, 1, true);
	view.setUint16(34, 8, true);
	text(36, 'data');
	view.setUint32(40, samples, true);
	new Uint8Array(buffer, 44).fill(128);
	return URL.createObjectURL(new Blob([buffer], { type: 'audio/wav' }));
}

async function loadedAudio(): Promise<HTMLAudioElement> {
	await expect.poll(() => document.querySelector('audio')?.readyState ?? 0).toBeGreaterThan(0);
	return document.querySelector('audio')!;
}

/** 自動再生で位置が進まないよう止めてから、位置を動かす。 */
async function pauseAt(seconds: number) {
	const audio = await loadedAudio();
	audio.pause();
	audio.currentTime = seconds;
	await expect
		.poll(() => (page.getByRole('slider').element() as HTMLInputElement).valueAsNumber)
		.toBe(seconds);
	return audio;
}

describe('AudioPlayerBar', () => {
	afterEach(() => {
		nowPlaying.stop();
		nowPlaying.autoAdvance = false;
		nowPlaying.muted = false;
	});

	it('mutes after the analyser so the spectrum keeps moving while muted', async () => {
		const first = { src: silentWavUrl(8), title: 'first' };
		await render(AudioPlayerBar);
		await userEvent.click(document.body);
		nowPlaying.play(first, [first]);
		const audio = await loadedAudio();
		await expect.poll(() => nowPlaying.analyser).not.toBeNull();
		expect(document.querySelector('canvas')).not.toBeNull();

		await page.getByRole('button', { name: /^(ミュート|Mute)$/ }).click();
		expect(nowPlaying.muted).toBe(true);
		expect(audio.muted).toBe(false);
	});

	it('starts the next track from the beginning, not from the previous position', async () => {
		const first = { src: silentWavUrl(8), title: 'first' };
		const second = { src: silentWavUrl(8), title: 'second' };
		await render(AudioPlayerBar);
		nowPlaying.play(first, [first, second]);

		await pauseAt(5);

		await page.getByRole('button', { name: /^(次の曲|Next track)$/ }).click();
		await expect.poll(() => document.querySelector('audio')?.src).toBe(second.src);
		expect((await loadedAudio()).currentTime).toBeLessThan(1);
	});

	it('restarts the current track when "previous" is pressed after 3 seconds', async () => {
		const first = { src: silentWavUrl(8), title: 'first' };
		const second = { src: silentWavUrl(8), title: 'second' };
		await render(AudioPlayerBar);
		nowPlaying.play(second, [first, second]);

		const audio = await pauseAt(5);

		await page.getByRole('button', { name: /^(前の曲|Previous track)$/ }).click();
		expect(nowPlaying.current).toEqual(second);
		expect(audio.currentTime).toBeLessThan(1);
	});

	it('plays a track chosen from a list', async () => {
		const first = { src: silentWavUrl(8), title: 'first' };
		await render(AudioPlayerBar);
		// 操作の無いページの再生はブラウザーに拒まれるので、一覧の行を押したときと同じく先に操作しておく。
		await userEvent.click(document.body);
		nowPlaying.play(first, [first]);

		await expect.poll(async () => (await loadedAudio()).paused).toBe(false);
		await expect.element(page.getByRole('button', { name: /^(一時停止|Pause)$/ })).toBeVisible();
	});

	it('keeps a paused player paused when moving to the next track', async () => {
		const first = { src: silentWavUrl(8), title: 'first' };
		const second = { src: silentWavUrl(8), title: 'second' };
		await render(AudioPlayerBar);
		nowPlaying.play(first, [first, second]);
		await pauseAt(1);

		await page.getByRole('button', { name: /^(次の曲|Next track)$/ }).click();
		await expect.poll(() => document.querySelector('audio')?.src).toBe(second.src);
		const audio = await loadedAudio();
		await new Promise((resolve) => setTimeout(resolve, 300));
		expect(audio.paused).toBe(true);
		await expect.element(page.getByRole('button', { name: /^(再生|Play)$/ })).toBeVisible();
	});

	it('plays the next track when "play next automatically" is on and a track ends', async () => {
		const first = { src: silentWavUrl(1), title: 'first' };
		const second = { src: silentWavUrl(8), title: 'second' };
		nowPlaying.autoAdvance = true;
		await render(AudioPlayerBar);
		await userEvent.click(document.body);
		nowPlaying.play(first, [first, second]);
		await pauseAt(0.5);
		(await loadedAudio()).play();

		await expect
			.poll(() => document.querySelector('audio')?.src, { timeout: 5000 })
			.toBe(second.src);
		await expect.poll(async () => (await loadedAudio()).paused).toBe(false);
		await expect.element(page.getByRole('button', { name: /^(一時停止|Pause)$/ })).toBeVisible();
	});

	it('resumes from the current position when the current track is chosen again', async () => {
		const first = { src: silentWavUrl(8), title: 'first' };
		await render(AudioPlayerBar);
		await userEvent.click(document.body);
		nowPlaying.play(first, [first]);
		const audio = await pauseAt(5);

		nowPlaying.play(first, [first]);
		await expect.poll(() => audio.paused).toBe(false);
		expect(audio.currentTime).toBeGreaterThanOrEqual(5);
	});

	it('shows the player as paused when the track cannot be loaded', async () => {
		await render(AudioPlayerBar);
		await userEvent.click(document.body);
		nowPlaying.play({ src: 'data:audio/wav;base64,AAAA', title: 'broken' });

		await expect.poll(() => nowPlaying.paused).toBe(true);
		await expect.element(page.getByRole('button', { name: /^(再生|Play)$/ })).toBeVisible();
	});

	it('ignores a late failure from the previous track once another track is playing', async () => {
		const first = { src: silentWavUrl(8), title: 'first' };
		const second = { src: silentWavUrl(8), title: 'second' };
		await render(AudioPlayerBar);
		await userEvent.click(document.body);
		const audio = document.querySelector('audio')!;
		const realPlay = audio.play.bind(audio);
		let rejectFirst: (error: unknown) => void = () => {};
		audio.play = () => new Promise<void>((_, reject) => (rejectFirst = reject));
		nowPlaying.play(first, [first, second]);
		audio.play = realPlay;

		await page.getByRole('button', { name: /^(次の曲|Next track)$/ }).click();
		await expect.poll(() => audio.paused).toBe(false);
		rejectFirst(new DOMException('failed', 'NotSupportedError'));
		await new Promise((resolve) => setTimeout(resolve, 100));
		expect(nowPlaying.paused).toBe(false);
	});
});
