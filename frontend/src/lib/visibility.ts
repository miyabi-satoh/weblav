/**
 * 管理画面で使う、公開範囲の判定。
 *
 * サーバーも同じ判定をする (→ docs/access.md「親が外れるときの公開範囲」・「ロールと操作」)。ここでの判定は、選べない値を見せず、
 * 実際の見え方を画面に示すためだけのもの。
 */
import type { components } from '$lib/api/schema';
import { selfAndAncestors } from '$lib/content-tree';

type ContentType = components['schemas']['ContentType'];
type Visibility = components['schemas']['Visibility'];

/** 厳しさの順 (→ docs/access.md「親が外れるときの公開範囲」)。 */
const STRICTNESS_ORDER: readonly Visibility[] = ['public', 'authenticated', 'private', 'hidden'];

/** 祖先を辿るのに要る、コンテンツの一部。 */
type VisibilityNode = { id: number; parentId?: number | null; visibility: Visibility };

function isStricter(a: Visibility, b: Visibility): boolean {
	return STRICTNESS_ORDER.indexOf(a) > STRICTNESS_ORDER.indexOf(b);
}

/**
 * 自分の公開範囲に、祖先のグループの公開範囲を重ねた実際の見え方 (→ docs/access.md「親が外れるときの公開範囲」)。
 * `private` はそのまま返す。見られる作成者はログイン済みで、祖先のグループもすべて開けるため。
 */
export function effectiveVisibility(own: Visibility, ancestors: readonly Visibility[]): Visibility {
	if (own === 'private') return own;
	return ancestors.reduce(
		(strictest, visibility) => (isStricter(visibility, strictest) ? visibility : strictest),
		own
	);
}

/** `parentId` のグループの下に置いたときの実際の見え方。祖先は `byId` から辿る。 */
export function effectiveVisibilityUnder<T extends VisibilityNode>(
	own: Visibility,
	parentId: number | null | undefined,
	byId: ReadonlyMap<number, T>
): Visibility {
	const ancestors =
		parentId == null ? [] : selfAndAncestors(parentId, byId).map((content) => content.visibility);
	return effectiveVisibility(own, ancestors);
}

/**
 * グループを削除すると、ルート直下へ移って実際の見え方が緩む直下の子 (→ docs/access.md「親が外れるときの公開範囲」)。
 * 公開範囲は書き換えないので、削除の確認で知らせるために使う。
 *
 * @returns 子と、削除前の実際の見え方。削除後の見え方は子の公開範囲そのもの。
 */
export function childrenLoosenedByDeletingGroup<T extends VisibilityNode>(
	groupId: number,
	contents: readonly T[],
	byId: ReadonlyMap<number, T>
): { content: T; before: Visibility }[] {
	return contents
		.filter((content) => content.parentId === groupId)
		.map((content) => ({
			content,
			before: effectiveVisibilityUnder(content.visibility, groupId, byId)
		}))
		.filter(({ content, before }) => isStricter(before, content.visibility));
}

/**
 * 親を付け替えると実際の見え方が緩むときの、前後の見え方 (→ docs/access.md「親が外れるときの公開範囲」)。
 * 公開範囲は書き換えないので、保存前の確認で知らせるために使う。
 * 親を変えないなら `null`。公開範囲だけを緩めるのは、操作した人が選んだ結果そのものなので知らせない。
 *
 * @param saved 保存済みの公開範囲と親。
 * @param next 保存しようとしている公開範囲と親。
 */
export function loosenedByReparent<T extends VisibilityNode>(
	saved: Pick<VisibilityNode, 'visibility' | 'parentId'>,
	next: Pick<VisibilityNode, 'visibility' | 'parentId'>,
	byId: ReadonlyMap<number, T>
): { before: Visibility; after: Visibility } | null {
	if ((saved.parentId ?? null) === (next.parentId ?? null)) return null;
	const before = effectiveVisibilityUnder(saved.visibility, saved.parentId, byId);
	const after = effectiveVisibilityUnder(next.visibility, next.parentId, byId);
	return isStricter(before, after) ? { before, after } : null;
}

/**
 * フォームで選べる公開範囲。並びは画面に出す順。
 *
 * @param editing 編集中のコンテンツ。新規作成なら `null` (作成者は操作した本人になる)。
 * @param userId 操作しているユーザー。
 */
export function selectableVisibilities(
	type: ContentType,
	editing: { visibility: Visibility; createdBy?: number | null } | null,
	userId: number | undefined
): Visibility[] {
	const isCreator = editing === null || (editing.createdBy != null && editing.createdBy === userId);
	if (editing?.visibility === 'private' && !isCreator) return ['private', 'hidden'];
	if (type === 'group' || !isCreator) return ['public', 'authenticated', 'hidden'];
	return ['public', 'authenticated', 'private', 'hidden'];
}
