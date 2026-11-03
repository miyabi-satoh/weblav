import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
import { withQuery } from '$lib/href';
import type { components } from '$lib/api/schema';

type UploadedFile = components['schemas']['UploadedFile'];

/** `POST /api/v1/shares/{id}/upload` の宛先。`path` は置き先のフォルダーの、共有からの相対パス。 */
export function shareUploadUrl(shareId: number, path: string): string {
	return withQuery(`/api/v1/shares/${shareId}/upload`, { path });
}

/** 1ファイルを送った結果。 */
export type UploadOutcome =
	{ ok: true; file: UploadedFile } | { ok: false; message: string; aborted: boolean };

/**
 * 1ファイルを1回の要求で送る。
 * 進み具合を出すため `fetch` ではなく XMLHttpRequest を使う (`fetch` は送る側の進み具合を返さない)。
 * `onProgress` には送ったバイトの割合 (0〜1) を渡す。`signal` で止めると、置きかけのファイルはサーバーが捨てる。
 */
export function uploadShareFile(
	url: string,
	file: File,
	onProgress: (ratio: number) => void,
	signal?: AbortSignal
): Promise<UploadOutcome> {
	return new Promise((resolve) => {
		const xhr = new XMLHttpRequest();
		xhr.open('POST', url);
		xhr.responseType = 'json';
		xhr.upload.onprogress = (event) => {
			if (event.lengthComputable && event.total > 0) onProgress(event.loaded / event.total);
		};
		xhr.onload = () => resolve(uploadOutcome(xhr.status, xhr.response));
		xhr.onerror = () => resolve({ ok: false, message: GENERIC_ERROR_MESSAGE(), aborted: false });
		xhr.onabort = () => resolve({ ok: false, message: '', aborted: true });
		signal?.addEventListener('abort', () => xhr.abort(), { once: true });
		if (signal?.aborted) {
			resolve({ ok: false, message: '', aborted: true });
			return;
		}
		const body = new FormData();
		body.append('file', file, file.name);
		xhr.send(body);
	});
}

/** 応答を結果に直す。1回に1ファイルを送るので、置いたものは `files` の先頭。 */
export function uploadOutcome(status: number, body: unknown): UploadOutcome {
	if (status >= 200 && status < 300) {
		const file = (body as { files?: UploadedFile[] } | null)?.files?.[0];
		if (file) return { ok: true, file };
		return { ok: false, message: GENERIC_ERROR_MESSAGE(), aborted: false };
	}
	return { ok: false, message: errorMessage(body), aborted: false };
}
