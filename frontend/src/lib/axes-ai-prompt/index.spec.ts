import { describe, expect, it } from 'vitest';
import { buildAxesPrompt, parseAxesAnswer, spread } from './index';

describe('spread', () => {
	it('keeps every item when there are few', () => {
		expect(spread([1, 2, 3], 5)).toEqual([1, 2, 3]);
	});

	it('picks items across the whole list', () => {
		expect(spread([0, 1, 2, 3, 4, 5, 6, 7, 8, 9], 5)).toEqual([0, 2, 4, 6, 8]);
	});
});

describe('buildAxesPrompt', () => {
	it('fills in the archive structure', () => {
		const prompt = buildAxesPrompt({
			itemCount: 2,
			levels: [{ level: 1, valueCount: 2, itemCount: 2, samples: [{ value: '2024', count: 1 }] }],
			words: [
				{ word: 'listening', count: 1, hiddenByDefault: false },
				{ word: '01', count: 2, hiddenByDefault: true }
			],
			relPaths: ['2024/listening.mp3', '2023/answer.pdf']
		});
		expect(prompt).toContain('- 1: 2 (2024)');
		expect(prompt).toContain('- listening: 1');
		expect(prompt).not.toContain('- 01: 2');
		expect(prompt.indexOf('- 2023/answer.pdf')).toBeLessThan(
			prompt.indexOf('- 2024/listening.mp3')
		);
		expect(prompt).not.toMatch(/\{\{\w+\}\}/);
	});
});

describe('parseAxesAnswer', () => {
	it('reads JSON wrapped in a code block and explanations', () => {
		const result = parseAxesAnswer(
			'Here you go:\n```json\n{"axes":[{"name":"Year","source":"dirLevel","dirLevel":1}]}\n```\nEnjoy.'
		);
		expect(result).toEqual({
			ok: true,
			value: { axes: [{ name: 'Year', source: 'dirLevel', dirLevel: 1 }] }
		});
	});

	it('prefers the code block when the explanation contains braces', () => {
		const result = parseAxesAnswer(
			'The title will look like {Year} {Type}.\n```json\n{"axes":[]}\n```\nThe {Year} axis uses folders.'
		);
		expect(result).toEqual({ ok: true, value: { axes: [] } });
	});

	it('rejects text that is not JSON', () => {
		expect(parseAxesAnswer('I cannot help with that.')).toEqual({ ok: false, reason: 'notJson' });
		expect(parseAxesAnswer('{"axes": [}')).toEqual({ ok: false, reason: 'notJson' });
	});

	it('rejects JSON without an axes list', () => {
		expect(parseAxesAnswer('{"titleTemplate":"{fileName}"}')).toEqual({
			ok: false,
			reason: 'notAxes'
		});
	});
});
