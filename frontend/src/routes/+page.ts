import { client } from '$lib/api/client';
import { fetchOrError, fetchOrNull } from '$lib/api/load';
import { parseBrowseSort } from '$lib/browse-sort';
import { viewAs } from '$lib/view-as.svelte';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// トップページは匿名でも開ける。一覧APIは閲覧レベルで絞った結果を返すため401にはならず、
// 未ログインなら `public` のコンテンツだけが並ぶ (→ docs/access.md「匿名閲覧の受け口」)。
// `parent()` を待ってから fetch するのは、ヘッダーのログイン状態表示に使う `user` の
// 取得と順序を揃えるため。
export const load: PageLoad = async ({ parent, url }) => {
	const { user } = await parent();

	// 並び順はURLクエリに持つ (→ docs/ui.md「ホーム・グループ・フォルダー・アーカイブの並び順」)。知らない値はサーバー側でも
	// 既定として扱われるが、選択UIの表示を合わせるためここでも読み直す。
	const sort = parseBrowseSort(url.searchParams.get('sort'));

	// 見出しに出すサイト設定も、未ログインで読める (→ docs/ui.md「UI 全般」)。
	// 共有はログインした人にだけ並べる (→ docs/ui.md「UI 全般」)。取れなくてもホームは出し、区画だけを出さない。
	// 閲覧者として見ている間は、ログインしていない人と同じく並べない。
	const [contents, siteSettings, shares] = await Promise.all([
		fetchOrError(
			client.GET('/api/v1/contents', { params: { query: { sort } } }),
			m.contents_fetch_failed()
		),
		fetchOrError(client.GET('/api/v1/site-settings'), m.site_settings_fetch_failed()),
		user && !viewAs.active ? fetchOrNull(client.GET('/api/v1/shares')) : null
	]);
	return { contents, siteSettings, sort, shares: shares ?? [] };
};
