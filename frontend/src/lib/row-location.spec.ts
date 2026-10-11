import FolderIcon from '@lucide/svelte/icons/folder';
import { describe, expect, it } from 'vitest';
import { rowLocationText, type RowLocation } from './row-location';

function location(labels: string[], from?: number): RowLocation {
	return {
		levels: labels.map((label) => ({ label, href: `/${label}`, icon: FolderIcon })),
		...(from === undefined ? {} : { from })
	};
}

describe('rowLocationText', () => {
	it('splits the parent from the levels above it', () => {
		expect(rowLocationText(location(['教材', '2024', '前期']))).toEqual({
			front: '教材 / 2024',
			tail: ' / 前期',
			full: '教材 / 2024 / 前期'
		});
	});

	it('puts a single level in the tail without a separator', () => {
		expect(rowLocationText(location(['ホーム']))).toEqual({
			front: '',
			tail: 'ホーム',
			full: 'ホーム'
		});
	});

	it('drops the levels above the search scope from the row but keeps them in the full path', () => {
		expect(rowLocationText(location(['教材', '2024', '前期', '音声'], 2))).toEqual({
			front: '前期',
			tail: ' / 音声',
			full: '教材 / 2024 / 前期 / 音声'
		});
	});

	it('keeps the scope itself for rows directly inside it', () => {
		expect(rowLocationText(location(['教材', '2024'], 1))).toEqual({
			front: '',
			tail: '2024',
			full: '教材 / 2024'
		});
		// 範囲が親より下を指すことは無いが、指しても場所を空にしない。
		expect(rowLocationText(location(['教材', '2024'], 5)).tail).toBe('2024');
	});
});
