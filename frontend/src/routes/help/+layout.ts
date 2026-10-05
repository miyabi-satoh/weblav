import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import { getLocale } from '$lib/paraglide/runtime';
import * as m from '$lib/paraglide/messages.js';
import type { LayoutLoad } from './$types';

// マニュアルは匿名で読める (→ docs/help.md)。ここではログイン状態を問わない。
// 本文の言語は画面の表示言語に従う。
export const load: LayoutLoad = async () => {
	const pages = await fetchOrError(
		client.GET('/api/v1/help/pages', { params: { query: { locale: getLocale() } } }),
		m.help_fetch_failed()
	);
	return { pages };
};
