import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// 共有はログインした人のもの。ホームの「共有」の区画と同じ行を並べ、パンくずの「共有」から戻る先にする。
export const load: PageLoad = async ({ parent, url }) => {
	await parent();
	const shares = await fetchOrError(client.GET('/api/v1/shares'), m.shares_fetch_failed(), {
		loginRedirectFrom: url
	});
	return { shares };
};
