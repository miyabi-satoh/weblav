import FolderIcon from '@lucide/svelte/icons/folder';
import { describe, expect, it } from 'vitest';
import { rowLocationText, scopeStart, type RowLocation } from './row-location';

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

describe('scopeStart', () => {
	it('is undefined without a scope', () => {
		expect(scopeStart(null, [1, 2], 3)).toBeUndefined();
	});

	it('starts at the group when the scope is one of the groups above', () => {
		expect(scopeStart({ contentId: 2, depth: 0 }, [1, 2], 3)).toBe(1);
		expect(scopeStart({ contentId: 1, depth: 0 }, [1, 2])).toBe(0);
	});

	it('starts at the folder level when the scope is inside the content itself', () => {
		// グループ2つの下のフォルダーの、2段下の階層。
		expect(scopeStart({ contentId: 3, depth: 2 }, [1, 2], 3)).toBe(4);
		// アーカイブやフォルダーそのものが範囲なら、そのコンテンツから。
		expect(scopeStart({ contentId: 3, depth: 0 }, [1, 2], 3)).toBe(2);
	});

	it('is undefined for rows outside the scope', () => {
		expect(scopeStart({ contentId: 9, depth: 1 }, [1, 2], 3)).toBeUndefined();
	});
});
