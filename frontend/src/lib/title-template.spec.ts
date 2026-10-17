import { describe, expect, it } from 'vitest';
import { removeAxisFromTemplate } from './title-template';

describe('removeAxisFromTemplate', () => {
	it('drops the separator after the axis when it comes first', () => {
		expect(removeAxisFromTemplate('{年度} {季節}', '年度')).toBe('{季節}');
	});

	it('drops the separator before the axis when something precedes it', () => {
		expect(removeAxisFromTemplate('{年度}・{季節}・{fileName}', '季節')).toBe('{年度}・{fileName}');
	});

	it('drops the separator before the axis when it comes last', () => {
		expect(removeAxisFromTemplate('{年度} - {季節}', '季節')).toBe('{年度}');
	});

	it('removes every occurrence of the axis', () => {
		expect(removeAxisFromTemplate('{年度} {季節} {年度}', '年度')).toBe('{季節}');
	});

	it('leaves text attached to the axis for the user to fix', () => {
		expect(removeAxisFromTemplate('第{回}回 {科目}', '回')).toBe('第回 {科目}');
	});

	it('returns an empty string when only the axis was used', () => {
		expect(removeAxisFromTemplate('{年度}', '年度')).toBe('');
	});

	it('does not touch an axis whose name only starts the same way', () => {
		expect(removeAxisFromTemplate('{年度} {年度末}', '年度')).toBe('{年度末}');
	});
});
