import { describe, expect, it } from 'vitest';
import type { components } from '#lib/api/schema.js';
import { addException, exceptionCandidates, visibleExceptions } from './root-access';

type UserListItem = components['schemas']['UserListItem'];

function user(id: number, username: string, role: UserListItem['role']): UserListItem {
	return { id, username, role, createdAt: '', privateContentCount: 0 } as UserListItem;
}

const admin = user(1, 'admin', 'admin');
const sato = user(2, 'sato', 'user');
const Abe = user(3, 'Abe', 'user');

describe('visibleExceptions', () => {
	const exceptions = [
		{ userId: 1, username: 'admin', access: 'none' as const },
		{ userId: 2, username: 'sato', access: 'add' as const }
	];

	it('hides exceptions of users who are now admins', () => {
		expect(visibleExceptions(exceptions, [admin, sato])).toEqual([exceptions[1]]);
	});

	it('shows every exception when the users could not be read', () => {
		expect(visibleExceptions(exceptions, null)).toEqual(exceptions);
	});
});

describe('exceptionCandidates', () => {
	it('offers only standard users without an exception', () => {
		const exceptions = [{ userId: 2, username: 'sato', access: 'add' as const }];
		expect(exceptionCandidates([admin, sato, Abe], exceptions)).toEqual([Abe]);
	});

	it('offers nobody when the users could not be read', () => {
		expect(exceptionCandidates(null, [])).toEqual([]);
	});
});

describe('addException', () => {
	it('keeps the server order, ignoring case', () => {
		const exceptions = [{ userId: 2, username: 'sato', access: 'add' as const }];
		expect(addException(exceptions, Abe, 'read').map((entry) => entry.username)).toEqual([
			'Abe',
			'sato'
		]);
	});
});
