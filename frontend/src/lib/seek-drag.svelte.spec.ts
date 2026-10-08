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

	it('keeps following the drag after the pointer leaves the bar until it is released', async () => {
		const onseek = vi.fn();
		mount(onseek);
		const rect = input.getBoundingClientRect();
		const y = rect.top + rect.height / 2;
		input.dispatchEvent(
			new PointerEvent('pointerdown', {
				pointerId: 1,
				button: 0,
				clientX: rect.left + 70,
				clientY: y,
				bubbles: true
			})
		);
		// バーの外 (下へ 100px) で左へ動かしても追う。
		window.dispatchEvent(
			new PointerEvent('pointermove', { pointerId: 1, clientX: rect.left + 30, clientY: y + 100 })
		);
		expect(onseek).toHaveBeenLastCalledWith(10);
		window.dispatchEvent(new PointerEvent('pointerup', { pointerId: 1 }));
		window.dispatchEvent(
			new PointerEvent('pointermove', { pointerId: 1, clientX: rect.left + 210 })
		);
		expect(onseek).toHaveBeenCalledTimes(2);
	});
});
