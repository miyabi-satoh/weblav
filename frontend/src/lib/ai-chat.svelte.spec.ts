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
