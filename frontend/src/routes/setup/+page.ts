import { client } from '$lib/api/client';
import type { PageLoad } from './$types';

/**
 * トークンが今も使えるかをサーバーに確かめる (→ docs/access.md「初回セットアップ」)。
 *
 * 使えない理由 (トークンが無い・違う・期限切れ・セットアップ済み) は区別せず、
 * 画面は同じ案内を出す。サーバーも同じ 404 を返す。
 */
export const load: PageLoad = async ({ url }) => {
	const token = url.searchParams.get('token') ?? '';
	if (token === '') return { token, valid: false };

	try {
		const { response } = await client.POST('/api/v1/setup/token/verify', { body: { token } });
		return { token, valid: response.ok };
	} catch {
		return { token, valid: false };
	}
};
