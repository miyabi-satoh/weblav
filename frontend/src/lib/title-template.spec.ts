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

	it('removes the brackets around the axis together with it', () => {
		expect(removeAxisFromTemplate('[{季節}] {場所}', '季節')).toBe('{場所}');
		expect(removeAxisFromTemplate('{年度}（{回}） {科目}', '回')).toBe('{年度} {科目}');
		expect(removeAxisFromTemplate('[({季節})] {場所}', '季節')).toBe('{場所}');
	});

	it('drops the separator after the axis when only separators precede it', () => {
		expect(removeAxisFromTemplate('- {年度} {季節}', '年度')).toBe('- {季節}');
	});

	it('keeps the brackets around the axis before the removed one', () => {
		expect(removeAxisFromTemplate('[{場所}] {季節}', '季節')).toBe('[{場所}]');
		expect(removeAxisFromTemplate('{年度} {学年} {号} [{教科}] {状態}', '状態')).toBe(
			'{年度} {学年} {号} [{教科}]'
		);
	});

	it('keeps brackets shared with another axis', () => {
		expect(removeAxisFromTemplate('[{教科}・{科目}]', '教科')).toBe('[{科目}]');
		expect(removeAxisFromTemplate('{年度} ({学年} {組})', '組')).toBe('{年度} ({学年})');
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
