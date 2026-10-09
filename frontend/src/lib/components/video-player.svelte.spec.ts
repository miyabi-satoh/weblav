import { afterEach, describe, expect, it } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import VideoPlayer from './video-player.svelte';
import { videoLoop } from '$lib/video-loop.svelte';

describe('VideoPlayer', () => {
	afterEach(() => {
		videoLoop.value = false;
	});

	it('loop button loops the video and is kept for the next video', async () => {
		const first = await render(VideoPlayer, { src: '', onerror: () => {} });
		const button = page.getByRole('button', { name: /^(繰り返し再生|Loop)$/ });
		await userEvent.click(button);

		expect(document.querySelector('video')?.loop).toBe(true);
		await expect.element(button).toHaveAttribute('aria-pressed', 'true');
		expect(localStorage.getItem('weblav:video-loop')).toBe('true');

		await first.unmount();
		await render(VideoPlayer, { src: '', onerror: () => {} });
		expect(document.querySelector('video')?.loop).toBe(true);
	});

	it('video keys work while focus is outside the player', async () => {
		await render(VideoPlayer, { src: '', onerror: () => {} });
		await userEvent.keyboard('m');

		expect(document.querySelector('video')?.muted).toBe(true);
	});
});
