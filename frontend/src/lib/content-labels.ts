/**
 * コンテンツの種類・公開範囲の表示ラベル。
 *
 * 網羅しなかった値は既定値に落とさず、値そのものをそのまま返す。既定値に落とすと、
 * 未対応の値が別の値として誤表示され、実装漏れに気付けない。
 */
import { formatNumber } from '$lib/format';
import * as m from '$lib/paraglide/messages.js';
import type { components } from '$lib/api/schema';

type ContentType = components['schemas']['ContentType'];
type Visibility = components['schemas']['Visibility'];

/**
 * 種類の呼び名。フォルダーを指すものは、一覧の表示を使うならライブラリ、使わないならショートカットと呼ぶ
 * (`listView`。→ docs/folders.md「呼び分け」)。ファイルを指すものもショートカット。
 */
export function contentTypeLabel(type: ContentType, listView = false): string {
	if (type === 'link') return m.contents_type_link();
	if (type === 'folder') return listView ? m.contents_type_library() : m.contents_type_shortcut();
	if (type === 'shortcut') return m.contents_type_shortcut();
	if (type === 'group') return m.contents_type_group();
	if (type === 'file') return m.contents_type_file();
	return type;
}

/** 作成ダイアログで種別を選ぶときに添える、一行の説明。 */
export function contentTypeDescription(type: ContentType): string {
	if (type === 'link') return m.contents_type_link_description();
	if (type === 'folder' || type === 'shortcut') return m.contents_type_shortcut_description();
	if (type === 'group') return m.contents_type_group_description();
	return type;
}

export function visibilityLabel(visibility: Visibility): string {
	if (visibility === 'public') return m.contents_visibility_public();
	if (visibility === 'authenticated') return m.contents_visibility_authenticated();
	if (visibility === 'private') return m.contents_visibility_private();
	if (visibility === 'hidden') return m.contents_visibility_hidden();
	return visibility;
}

/** ライブラリのアイテムの数。一覧・編集ページの見出しで同じ言い方にする。 */
export function archiveItemsSummary(counts: { total: number }): string {
	return m.archive_items_summary({ count: formatNumber(counts.total) });
}
