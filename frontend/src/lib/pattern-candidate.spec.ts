import { describe, expect, it } from 'vitest';
import { appendPattern, patternCandidate } from './pattern-candidate';

describe('patternCandidate', () => {
	it('wraps the words with asterisks', () => {
		expect(patternCandidate('jpg')).toBe('*jpg*');
		expect(patternCandidate('英語　2024')).toBe('*英語*2024*');
		expect(patternCandidate('  ')).toBe('');
	});

	it('matches at any depth when a word has a slash', () => {
		expect(patternCandidate('2024/eng')).toBe('**/*2024/eng*');
	});

	it('keeps symbols in the words literal', () => {
		expect(patternCandidate('[mix]')).toBe('*[[]mix[]]*');
		expect(patternCandidate('a*b?')).toBe('*a[*]b[?]*');
		expect(patternCandidate('{x}')).toBe('*[{]x[}]*');
	});
});

describe('appendPattern', () => {
	it('adds a new line and skips one already there', () => {
		expect(appendPattern(null, '*.pdf')).toBe('*.pdf');
		expect(appendPattern('*.tmp\n', '*.pdf')).toBe('*.tmp\n*.pdf');
		expect(appendPattern('*.pdf', '*.pdf')).toBe('*.pdf');
	});
});
