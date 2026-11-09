import { error } from '@sveltejs/kit';
import { GENERIC_ERROR_MESSAGE } from '$lib/api/errors';
import type { PageLoad } from './$types';

// ログインしていなければ、ルートの `+layout.ts` がログインへ送る (`isProtectedRoute`)。
// 名前を `user` にしない。親の `user` (null を含む型) を、null でない型で受け直すため。
export const load: PageLoad = async ({ parent }) => {
	const { user } = await parent();
	if (user === null) error(500, GENERIC_ERROR_MESSAGE());
	return { me: user };
};
