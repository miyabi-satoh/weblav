import { describe, expect, it } from 'vitest';
import { spectrumBars } from './spectrum';

describe('spectrumBars', () => {
	it('returns one height per bar, scaled to 0-1', () => {
		const bars = spectrumBars(new Uint8Array(1024).fill(255), 48000, 12);
		expect(bars).toHaveLength(12);
		expect(bars.every((bar) => bar === 1)).toBe(true);
	});

	it('returns zero heights for silence', () => {
		expect(spectrumBars(new Uint8Array(1024), 48000, 12).every((bar) => bar === 0)).toBe(true);
	});

	it('puts a low tone in the first bar and a high tone in the last bar', () => {
		const binHz = 48000 / 2 / 1024;
		const tone = (hz: number) => {
			const data = new Uint8Array(1024);
			data[Math.round(hz / binHz)] = 200;
			return spectrumBars(data, 48000, 12);
		};
		const low = tone(70);
		expect(low[0]).toBeGreaterThan(0);
		expect(low.slice(1).every((bar) => bar === 0)).toBe(true);
		const high = tone(15000);
		expect(high[11]).toBeGreaterThan(0);
		expect(high.slice(0, 11).every((bar) => bar === 0)).toBe(true);
	});
});
