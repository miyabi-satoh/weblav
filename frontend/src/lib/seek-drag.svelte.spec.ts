import { afterEach, describe, expect, it, vi } from 'vitest';
import { userEvent } from 'vitest/browser';
import { seekDrag } from './seek-drag';

describe('seekDrag', () => {
	let cleanup: (() => void) | void;
	let input: HTMLInputElement;

	afterEach(() => {
		cleanup?.();
		input.remove();
	});

	function mount(onseek: (seconds: number) => void) {
		input = document.createElement('input');
		input.type = 'range';
		input.max = '100';
		input.style.width = '220px';
		document.body.append(input);
		cleanup = seekDrag(onseek)(input);
	}

	it('seeks to where the bar is pressed and keeps the focus on the bar', async () => {
		const onseek = vi.fn();
		mount(onseek);
		// 左端からつまみの半分 (10px) + 残り 200px の 30% の位置。
		await userEvent.click(input, { position: { x: 70, y: 5 } });
		expect(onseek).toHaveBeenCalledOnce();
		// 押した位置は小数の px に丸められて届く。
		expect(onseek.mock.calls[0][0]).toBeCloseTo(30, 0);
		// 続く左右キーをシークバーが受ける。
		expect(document.activeElement).toBe(input);
	});
});
