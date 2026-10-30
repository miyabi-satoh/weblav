import { client } from '$lib/api/client';
import { fetchOrError } from '$lib/api/load';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// 非表示のコンテンツは閲覧の場のどこにも並ばないので、戻す・直す・消す入口をここに集める。
// 管理画面の一覧も取るのは、削除の確認で、グループを消すと見える範囲が広がる子を知らせるため
// (→ content-delete-dialog.svelte)。
export const load: PageLoad = async ({ parent, url }) => {
	await parent();
	const [hidden, contents] = await Promise.all([
		fetchOrError(client.GET('/api/v1/contents/hidden'), m.hidden_fetch_failed(), {
			loginRedirectFrom: url
		}),
		fetchOrError(client.GET('/api/v1/admin/contents'), m.contents_fetch_failed(), {
			loginRedirectFrom: url
		})
	]);
	return { hidden, contents };
};
