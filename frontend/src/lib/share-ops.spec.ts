import { describe, expect, it } from 'vitest';
import { accessAtLeast, canAddTo, childSharePath, lastSegment, shareRowActions } from './share-ops';

const file = { isDir: false, listed: false };
const dir = { isDir: true, listed: false };

describe('shareRowActions', () => {
	it('offers only reading actions where the viewer can only read', () => {
		expect(shareRowActions('read', file, true)).toEqual(['copy', 'download', 'listOnHome']);
		expect(shareRowActions('read', dir, true)).toEqual(['copy', 'listOnHome']);
	});

	it('does not offer rename, move or delete where the viewer can only add', () => {
		expect(shareRowActions('add', file, true)).toEqual(['copy', 'download', 'listOnHome']);
	});

	it('offers every action where the viewer can do everything', () => {
		expect(shareRowActions('all', file, true)).toEqual([
			'rename',
			'move',
			'copy',
			'download',
			'listOnHome',
			'delete'
		]);
		expect(shareRowActions('all', dir, true)).toEqual([
			'rename',
			'move',
			'copy',
			'listOnHome',
			'delete'
		]);
	});

	it('drops copy when no share accepts additions', () => {
		expect(shareRowActions('all', file, false)).not.toContain('copy');
	});

	it('does not offer listing a folder that is already listed', () => {
		expect(shareRowActions('read', { isDir: true, listed: true }, true)).toEqual(['copy']);
		expect(shareRowActions('read', { isDir: false, listed: true }, true)).toEqual([
			'copy',
			'download'
		]);
	});
});

describe('access helpers', () => {
	it('orders access levels like the server', () => {
		expect(accessAtLeast('all', 'add')).toBe(true);
		expect(accessAtLeast('add', 'all')).toBe(false);
		expect(accessAtLeast('none', 'read')).toBe(false);
		expect(canAddTo('read')).toBe(false);
		expect(canAddTo('add')).toBe(true);
	});
});

describe('share paths', () => {
	it('joins a name onto the folder path', () => {
		expect(childSharePath('', 'a.txt')).toBe('a.txt');
		expect(childSharePath('教材/英検', 'a.txt')).toBe('教材/英検/a.txt');
	});

	it('names the share itself by the fallback', () => {
		expect(lastSegment('', '教材')).toBe('教材');
		expect(lastSegment('教材/英検', '教材')).toBe('英検');
	});
});
