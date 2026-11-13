import { error, redirect } from '@sveltejs/kit';
import { resolve } from '$app/paths';
import { client } from '#lib/api/client.ts';
import { fetchOrError } from '#lib/api/load.ts';
import { canEditContent } from '#lib/auth.ts';
import { contentSelfHref } from '#lib/content-view-href.ts';
import * as m from '#lib/paraglide/messages.js';
import type { LayoutLoad } from './$types';

// 単一コンテンツ取得APIが無いため、一覧を取って対象idを探す。
// 一覧そのものも、編集ページが親グループの候補と公開範囲の判定に使う。
export const load: LayoutLoad = async ({ parent, params, url }) => {
	const { user } = await parent();

	const id = Number(params.id);

	const contents = await fetchOrError(
		client.GET('/api/v1/contents/manage'),
		m.contents_fetch_failed(),
		{ loginRedirectFrom: url }
	);
	const content = contents.find((c) => c.id === id);
	if (!content) {
		error(404, m.error_not_found());
	}

	// 書き換えられない人は、そのコンテンツの閲覧画面へ送る (→ docs/ui.md「UI 全般」)。外へのリンクはホームへ。
	if (!canEditContent(user, content)) {
		const view = contentSelfHref(content);
		redirect(303, view.external ? resolve('/') : view.href);
	}

	return { contents, content, contentId: id };
};
