import { describe, expect, it } from 'vitest';
import {
	axisValuesPreviewBlocker,
	FILENAME_WORD_VALUES_LIMIT,
	wordCandidates
} from './axis-value-assist';

const words = [
	{ word: 'listening', count: 4, hiddenByDefault: false },
	{ word: 'English', count: 3, hiddenByDefault: false },
	{ word: '2024', count: 2, hiddenByDefault: true },
	{ word: 'answer', count: 1, hiddenByDefault: true }
];

describe('wordCandidates', () => {
	it('leaves out hidden words when there is no query', () => {
		expect(wordCandidates(words, [], '').map((w) => w.word)).toEqual(['listening', 'English']);
		expect(wordCandidates(words, [], '  ').map((w) => w.word)).toEqual(['listening', 'English']);
	});

	it('finds hidden words too by a part of the word, ignoring case', () => {
		expect(wordCandidates(words, [], '20').map((w) => w.word)).toEqual(['2024']);
		expect(wordCandidates(words, [], 'EN').map((w) => w.word)).toEqual(['listening', 'English']);
		expect(wordCandidates(words, [], 'ans').map((w) => w.word)).toEqual(['answer']);
	});

	it('leaves out words already in the table, ignoring case and surrounding spaces', () => {
		expect(wordCandidates(words, [' english ', 'ANSWER'], '').map((w) => w.word)).toEqual([
			'listening'
		]);
		expect(wordCandidates(words, ['ANSWER'], 'ans')).toHaveLength(0);
	});

	it('ignores empty rows in the table', () => {
		expect(wordCandidates(words, ['', '  '], '')).toHaveLength(2);
	});
});

describe('axisValuesPreviewBlocker', () => {
	it('accepts an empty table and distinct words', () => {
		expect(axisValuesPreviewBlocker([], 'filenameWord')).toBeNull();
		expect(axisValuesPreviewBlocker(['listening', 'answer'], 'filenameWord')).toBeNull();
	});

	it('rejects a table with an empty row', () => {
		expect(axisValuesPreviewBlocker(['listening', ' '], 'filenameWord')).toBe('incomplete');
	});

	it('rejects duplicates after trimming, but treats a case difference as distinct like the server', () => {
		expect(axisValuesPreviewBlocker(['answer', ' answer'], 'filenameWord')).toBe('incomplete');
		expect(axisValuesPreviewBlocker(['answer', 'Answer'], 'filenameWord')).toBeNull();
	});

	it('limits the number of words only on filename word axes', () => {
		const words = (count: number) => Array.from({ length: count }, (_, i) => `w${i}`);
		expect(axisValuesPreviewBlocker(words(FILENAME_WORD_VALUES_LIMIT), 'filenameWord')).toBeNull();
		expect(axisValuesPreviewBlocker(words(FILENAME_WORD_VALUES_LIMIT + 1), 'filenameWord')).toBe(
			'tooMany'
		);
		expect(axisValuesPreviewBlocker(words(FILENAME_WORD_VALUES_LIMIT + 1), 'dirLevel')).toBeNull();
	});
});
