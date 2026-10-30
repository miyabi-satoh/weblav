import { describe, expect, it } from 'vitest';
import { canPlaceAt } from './share-transfer-dialog.svelte';

const source = { shareId: 1, dirPath: '教材', target: { path: '教材/英検', isDir: true } };
const at = (shareId: number, path: string, access: 'read' | 'add' | 'all' = 'all') => ({
	shareId,
	shareName: 's',
	path,
	access
});

describe('canPlaceAt', () => {
	it('refuses places where the viewer cannot add', () => {
		expect(canPlaceAt('copy', source, at(2, '', 'read'))).toBe(false);
		expect(canPlaceAt('copy', source, at(2, '', 'add'))).toBe(true);
		expect(canPlaceAt('copy', source, null)).toBe(false);
	});

	it('refuses a folder itself and anything inside it', () => {
		expect(canPlaceAt('copy', source, at(1, '教材/英検'))).toBe(false);
		expect(canPlaceAt('copy', source, at(1, '教材/英検/2024'))).toBe(false);
		// 名前が前方で重なるだけの別のフォルダーには置ける。
		expect(canPlaceAt('copy', source, at(1, '教材/英検2'))).toBe(true);
	});

	it('allows copying into the same folder but not moving there', () => {
		expect(canPlaceAt('copy', source, at(1, '教材'))).toBe(true);
		expect(canPlaceAt('move', source, at(1, '教材'))).toBe(false);
	});
});
