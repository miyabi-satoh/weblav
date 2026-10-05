import { error } from '@sveltejs/kit';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// アーカイブのアイテム一覧。アイテムは ArchiveItemList が自分で取り、再スキャン・公開の切り替えは
// user にも開いている (→ docs/access.md「ロールと操作」)。対象のコンテンツは [id]/+layout.ts が一覧から探して渡す。
export const load: PageLoad = async ({ parent }) => {
	const { content } = await parent();
	if (content.type !== 'archive') {
		error(404, m.error_not_found());
	}
	return {};
};
