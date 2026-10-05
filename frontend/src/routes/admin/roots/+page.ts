import { error } from '@sveltejs/kit';
import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import { isLoopbackHost } from '$lib/loopback';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// 可否を決めるのは API 側 (admin かつサーバーの PC からの要求、→ docs/folders.md「公開できるフォルダ」)。
// LAN の端末から開くと API は 404 になる。タブもその場合は出さない (+layout.svelte)。
// ブックマークなどで開かれたときは、消えたページと取り違えないよう、ここで開けない理由を出す。
// 画面は Host で LAN と分かっているので、伏せる対象 (API の口) を明かすことにはならない。
export const load: PageLoad = async ({ parent, url }) => {
	await parent();
	if (!isLoopbackHost(url.hostname)) {
		error(403, m.admin_roots_server_only());
	}

	const roots = await fetchOrError(
		client.GET('/api/v1/admin/roots'),
		m.admin_roots_fetch_failed(),
		{ loginRedirectFrom: url }
	);
	return { roots };
};
