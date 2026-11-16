/**
 * 共有の追加と場所の選び直しで、サーバーの PC に OS のフォルダー選択の窓を出す (→ docs/folders.md「選び方」)。
 *
 * 窓は何分でも開いていられるが、1回の要求は窓が閉じるのを長くは待たない (ブラウザーが待ちを打ち切るため)。
 * 「まだ開いている」と返ったら、同じ窓の結果を待ち直す。
 */
import { client } from '#lib/api/client.ts';
import { GENERIC_ERROR_MESSAGE } from '#lib/api/errors.ts';
import type { ErrorDialogState } from '#lib/error-dialog-state.svelte.ts';
import * as m from '#lib/paraglide/messages.js';

/** 窓で選ばれたパス。キャンセルなら `null`。失敗も `null` で、理由は `errorDialog` に出す。 */
export async function pickRootFolder(errorDialog: ErrorDialogState): Promise<string | null> {
	try {
		const { data, error, response } = await client.POST('/api/v1/admin/roots/pick');
		if (!response.ok || !data) {
			// 窓がほかの画面から開かれたまま。ブラウザーの後ろに隠れていることもあるので、そう伝える。
			if (response.status === 409) errorDialog.show(m.admin_roots_picker_already_open());
			else errorDialog.showError(error);
			return null;
		}
		let pick = data;
		while (pick.open) {
			const next = await waitAgain(pick.id);
			if ('failed' in next) {
				errorDialog.showError(next.failed);
				return null;
			}
			pick = next;
		}
		return pick.path ?? null;
	} catch {
		errorDialog.show(GENERIC_ERROR_MESSAGE());
	}
	return null;
}

/**
 * 窓 `id` の結果を待ち直す。通信が一度切れても、もう一度だけ待ち直す
 * (待っている間に回線が瞬断しても、閉じた窓の結果はサーバーに残っているため)。
 */
async function waitAgain(id: number) {
	const request = () => client.GET('/api/v1/admin/roots/pick/{id}', { params: { path: { id } } });
	let result;
	try {
		result = await request();
	} catch {
		result = await request();
	}
	const { data, error } = result;
	return data ?? { failed: error };
}
