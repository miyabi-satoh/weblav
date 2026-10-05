/**
 * コンテンツの種類・公開範囲の表示ラベル。
 *
 * 網羅しなかった値は既定値に落とさず、値そのものをそのまま返す。既定値に落とすと、
 * 未対応の値が別の値として誤表示され、実装漏れに気付けない。
 */
import * as m from '$lib/paraglide/messages.js';
import type { components } from '$lib/api/schema';

type ContentType = components['schemas']['ContentType'];
type Visibility = components['schemas']['Visibility'];

export function contentTypeLabel(type: ContentType): string {
	if (type === 'link') return m.contents_type_link();
	if (type === 'folder') return m.contents_type_folder();
	if (type === 'group') return m.contents_type_group();
	if (type === 'file') return m.contents_type_file();
	if (type === 'archive') return m.contents_type_archive();
	return type;
}

/** 作成ダイアログで種別を選ぶときに添える、一行の説明。 */
export function contentTypeDescription(type: ContentType): string {
	if (type === 'link') return m.contents_type_link_description();
	if (type === 'folder') return m.contents_type_folder_description();
	if (type === 'group') return m.contents_type_group_description();
	if (type === 'file') return m.contents_type_file_description();
	if (type === 'archive') return m.contents_type_archive_description();
	return type;
}

export function visibilityLabel(visibility: Visibility): string {
	if (visibility === 'public') return m.contents_visibility_public();
	if (visibility === 'authenticated') return m.contents_visibility_authenticated();
	if (visibility === 'private') return m.contents_visibility_private();
	if (visibility === 'hidden') return m.contents_visibility_hidden();
	return visibility;
}
