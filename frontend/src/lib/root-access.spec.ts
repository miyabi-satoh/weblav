import { describe, expect, it } from 'vitest';
import { accessExceptions } from './root-access-dialog.svelte';

describe('accessExceptions', () => {
	it('sends only users with a chosen exception', () => {
		expect(accessExceptions({ 2: 'default', 3: 'add', 4: 'none' })).toEqual([
			{ userId: 3, access: 'add' },
			{ userId: 4, access: 'none' }
		]);
	});
});
