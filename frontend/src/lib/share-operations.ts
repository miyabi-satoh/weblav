import * as m from '$lib/paraglide/messages.js';
import type { components } from '$lib/api/schema';

type ShareOperation = components['schemas']['ShareOperationResponse'];
type ShareOp = components['schemas']['ShareOp'];

/** 操作の記録の、操作の名前。 */
export function shareOpLabel(op: ShareOp): string {
	switch (op) {
		case 'upload':
			return m.admin_operations_op_upload();
		case 'mkdir':
			return m.admin_operations_op_mkdir();
		case 'rename':
			return m.admin_operations_op_rename();
		case 'move':
			return m.admin_operations_op_move();
		case 'copy':
			return m.admin_operations_op_copy();
		case 'delete':
			return m.admin_operations_op_delete();
		case 'list_on_home':
			return m.admin_operations_op_list_on_home();
	}
}

/** 済んだもの以外の結果の印。済んだものは印を付けない (ほとんどが済んだもので、並べると目立たなくなるため)。 */
export function shareOpResultLabel(operation: ShareOperation): string | undefined {
	if (operation.result === 'running') return m.admin_operations_result_running();
	if (operation.result === 'failed') return m.admin_operations_result_failed();
	if (operation.result === 'cancelled') return m.admin_operations_result_cancelled();
	return undefined;
}

/** 共有の中の場所を「共有の名前 / パス」で書く。共有そのものは名前だけ。 */
export function sharePlace(shareName: string, path: string): string {
	return path === '' ? shareName : `${shareName} / ${path}`;
}

/** 扱った場所。いくつもあれば「、」でつなぐ。置き先があれば「→」の後に書く。 */
export function shareOpPlaces(operation: ShareOperation): { from: string; to?: string } {
	const from = operation.paths.map((path) => sharePlace(operation.shareName, path)).join('、');
	if (operation.destShareName == null) return { from };
	return { from, to: sharePlace(operation.destShareName, operation.destPath ?? '') };
}
