import type { components } from '#lib/api/schema.js';
import { formatNumber } from '#lib/format.ts';
import * as m from '#lib/paraglide/messages.js';

type VisibilityPreview = components['schemas']['VisibilityPreview'];

/** パターンを変えたときに、見えるようになる・見えなくなるファイルの数の文。増減の無い側は出さない。 */
export function visibilityChangeCounts(change: VisibilityPreview): string[] {
	const lines: string[] = [];
	if (change.gained > 0) {
		lines.push(m.contents_edit_visibility_change_gained({ count: formatNumber(change.gained) }));
	}
	if (change.lost > 0) {
		lines.push(m.contents_edit_visibility_change_lost({ count: formatNumber(change.lost) }));
	}
	return lines;
}
