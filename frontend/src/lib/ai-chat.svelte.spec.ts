import { flushSync } from 'svelte';
import { describe, expect, it } from 'vitest';
import { aiChat } from './ai-chat.svelte';

describe('aiChat.recheck', () => {
	it('can be called from an effect without making the effect run again', () => {
		let runs = 0;
		const before = aiChat.checks;
		const cleanup = $effect.root(() => {
			$effect(() => {
				runs += 1;
				aiChat.recheck();
			});
		});
		flushSync();
		cleanup();
		expect(runs).toBe(1);
		expect(aiChat.checks).toBe(before + 1);
	});
});

describe('aiChat.setFile', () => {
	const pdf = { name: 'a.pdf', src: '/a', sendsContent: true };
	const photo = { name: 'b.jpg', src: '/b', sendsContent: true };

	it('keeps the file another viewer passed when one viewer closes', () => {
		aiChat.setFile('file', pdf);
		// 隣の画像へ渡すと、画像のビューアーが開いてから、ファイルのビューアーが閉じる。
		aiChat.setFile('image', photo);
		aiChat.setFile('file', null);
		expect(aiChat.file).toEqual(photo);
		aiChat.setFile('image', null);
		expect(aiChat.file).toBeNull();
	});
});
