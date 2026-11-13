/**
 * 共有の追加と場所の選び直しで、サーバーの PC に OS のフォルダー選択の窓を出す (→ docs/folders.md「選び方」)。
 */
import { client } from '#lib/api/client.ts';
import { GENERIC_ERROR_MESSAGE } from '#lib/api/errors.ts';
import type { ErrorDialogState } from '#lib/error-dialog-state.svelte.ts';
import * as m from '#lib/paraglide/messages.js';

/** 窓で選ばれたパス。キャンセルなら `null`。失敗も `null` で、理由は `errorDialog` に出す。 */
export async function pickRootFolder(errorDialog: ErrorDialogState): Promise<string | null> {
	try {
		const { data, error, response } = await client.POST('/api/v1/admin/roots/pick');
		if (response.ok) return data?.path ?? null;
		// 窓がほかの画面から開かれたまま。ブラウザーの後ろに隠れていることもあるので、そう伝える。
		if (response.status === 409) errorDialog.show(m.admin_roots_picker_already_open());
		else errorDialog.showError(error);
	} catch {
		errorDialog.show(GENERIC_ERROR_MESSAGE());
	}
	return null;
}
