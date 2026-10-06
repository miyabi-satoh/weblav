import { client } from '$lib/api/client';
import { errorMessage } from '$lib/api/errors';
import { fetchOrError, fetchOrNull } from '$lib/api/load';
import * as m from '$lib/paraglide/messages.js';
import type { PageLoad } from './$types';

// admin 限定であることは +layout.ts が確認済み。サイト設定の読み取りはホームと同じ API を使う。
export const load: PageLoad = async ({ parent, url }) => {
	await parent();

	const [settings, server, logSettings, pro, health] = await Promise.all([
		fetchOrError(client.GET('/api/v1/site-settings'), m.site_settings_fetch_failed(), {
			loginRedirectFrom: url
		}),
		loadServerSettings(),
		fetchOrNull(client.GET('/api/v1/admin/log-settings')),
		fetchOrNull(client.GET('/api/v1/admin/pro')),
		fetchOrNull(client.GET('/api/v1/health'))
	]);
	return { settings, ...server, logSettings, pro, health };
};

// サーバーの設定は、取れなくても画面ごと出せなくはしない。config.toml を手で壊したとき (409) も、
// 「ホームの表示」は変えられるようにし、直し方をその区画に出すため。
async function loadServerSettings() {
	try {
		const { data, error, response } = await client.GET('/api/v1/admin/server-settings');
		if (response.ok && data) return { serverSettings: data, serverSettingsError: null };
		const message =
			response.status === 409 ? errorMessage(error) : m.admin_settings_server_fetch_failed();
		return { serverSettings: null, serverSettingsError: message };
	} catch {
		return { serverSettings: null, serverSettingsError: m.admin_settings_server_fetch_failed() };
	}
}
