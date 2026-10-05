import { client } from '$lib/api/client';
import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
import type { components } from '$lib/api/schema';

type RescanResponse = components['schemas']['RescanResponse'];

export type ArchiveScanResult =
	{ ok: true; result: RescanResponse } | { ok: false; message: string };

/**
 * アーカイブを再スキャンする。失敗は投げずに、画面に出す文言にして返す。
 *
 * 作成・場所の変更の直後にも呼ぶ (→ docs/archive.md「スキャン」)。その場合は作成・保存が
 * 済んだ後の失敗なので、呼び出し側が「保存はできた」ことと分けて伝える。
 */
export async function rescanArchive(id: number): Promise<ArchiveScanResult> {
	try {
		const { data, error, response } = await client.POST('/api/v1/contents/{id}/rescan', {
			params: { path: { id } }
		});
		if (!response.ok || !data) return { ok: false, message: errorMessage(error) };
		return { ok: true, result: data };
	} catch {
		return { ok: false, message: GENERIC_ERROR_MESSAGE() };
	}
}
