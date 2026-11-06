import { resolve } from '$app/paths';
import { isAdmin } from '$lib/auth';
import * as m from '$lib/paraglide/messages.js';
import type { components } from '$lib/api/schema';

type Content = components['schemas']['AdminContentResponse'];
type User = Pick<components['schemas']['UserResponse'], 'role'> | null | undefined;

/**
 * コンテンツの編集ページの戻り先。削除した後もここへ移る。
 * admin はコンテンツの管理の一覧、user は閲覧の場でこのコンテンツが並ぶ所 (親のグループかホーム)。
 * 管理画面は admin 専用で、user は閲覧の画面の ⋮ から来るため (→ docs/access.md「ロールと操作」)。
 */
export function contentEditBack(
	user: User,
	content: Pick<Content, 'parentId'>,
	contentById: ReadonlyMap<number, Content>
): { href: string; label: string } {
	if (isAdmin(user)) {
		return { href: resolve('/admin/contents'), label: m.admin_contents_nav_title() };
	}
	const parent = content.parentId == null ? undefined : contentById.get(content.parentId);
	if (parent) {
		return { href: resolve('/groups/[id]', { id: String(parent.id) }), label: parent.title };
	}
	return { href: resolve('/'), label: m.breadcrumb_home() };
}
