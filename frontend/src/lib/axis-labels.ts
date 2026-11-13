import * as m from '#lib/paraglide/messages.js';
import type { components } from '#lib/api/schema.js';

type AxisSource = components['schemas']['AxisSource'];

/** 分類の抽出元の名前 (分類の編集の選択肢・操作の記録)。 */
export function axisSourceLabel(source: AxisSource): string {
	if (source === 'dirLevel') return m.archive_axes_source_option_dir_level();
	if (source === 'dateYear') return m.archive_axes_source_date_year();
	if (source === 'dateMonth') return m.archive_axes_source_date_month();
	return m.archive_axes_source_option_filename_word();
}
