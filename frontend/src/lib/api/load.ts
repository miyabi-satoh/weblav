import { error, redirect } from '@sveltejs/kit';
import { resolve } from '$app/paths';
import { GENERIC_ERROR_MESSAGE, errorCode } from '$lib/api/errors';
import { loginPathWithRedirect } from '$lib/auth';
import * as m from '$lib/paraglide/messages.js';

type Options = {
	loginRedirectFrom?: URL;
};

/**
 * 取れなければ null を返す。取れなくても画面ごと出せなくはせず、その区画に理由だけを出すときに使う。
 */
export async function fetchOrNull<T>(
	request: Promise<{ data?: T; response: Response }>
): Promise<T | null> {
	try {
		const { data, response } = await request;
		return response.ok && data != null ? data : null;
	} catch {
		return null;
	}
}

/** ルートの `+layout.ts` は `/auth/me` の 401 を `user: null` として扱うため、これを使わない。 */
export async function fetchOrError<T>(
	request: Promise<{ data?: T; error?: unknown; response: Response }>,
	failedMessage: string,
	{ loginRedirectFrom }: Options = {}
): Promise<T> {
	let response: Response;
	let data: T | undefined;
	let body: unknown;
	try {
		({ data, error: body, response } = await request);
	} catch {
		error(503, GENERIC_ERROR_MESSAGE());
	}
	if (loginRedirectFrom && response.status === 401) {
		redirect(
			307,
			loginPathWithRedirect(
				resolve('/login'),
				loginRedirectFrom.pathname + loginRedirectFrom.search
			)
		);
	}
	if (response.status === 404) {
		error(404, m.error_not_found());
	}
	// 画面ごとの「読み込めませんでした」では、NAS やディスクを確かめればよいと分からないため。
	if (errorCode(body) === 'folder_unreachable') {
		error(response.status, m.error_folder_unreachable());
	}
	if (!response.ok || data == null) {
		// error() は 400〜599 しか受け付けないため、200番台なのに data が無い想定外のケースは 500 とする。
		error(response.ok ? 500 : response.status, failedMessage);
	}
	return data;
}
