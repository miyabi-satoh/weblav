import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import { getLocale } from '$lib/paraglide/runtime';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ params }) => {
	const helpPage = await fetchOrError(
		client.GET('/api/v1/help/pages/{slug}', {
			params: { path: { slug: params.slug }, query: { locale: getLocale() } }
		}),
		m.help_fetch_failed()
	);
	return { helpPage };
};
