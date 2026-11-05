import createClient from 'openapi-fetch';
import { goto } from '$app/navigation';
import { navigating } from '$app/state';
import { resolve } from '$app/paths';
import { loginPathWithRedirect } from '$lib/auth';
import { errorCode } from '$lib/api/errors';
import type { paths } from '$lib/api/schema';

// openapi.json のパスは `/api/v1/...` を含む (backend 側で nest した prefix がそのまま
// ドキュメントに載る) ため、baseUrl は空で良い。
export const client = createClient<paths>({ baseUrl: '' });

/** `client` の要求が返す形のうち、画面が見る部分。要求を引数で受け取る部品の型に使う。 */
export type ApiResult<T = unknown> = { data?: T; error?: unknown; response: Response };

/**
 * 401 でもログインへ送らないエンドポイント。未ログインで呼ぶのが正常で、
 * 401 は呼び出し側が扱う。
 */
const ANONYMOUS_PATHS = ['/api/v1/auth/me', '/api/v1/auth/login', '/api/v1/auth/recover'];

/**
 * 画面を開いたまま操作したときの 401 をログインへ繋ぐ。
 *
 * 送るのはログインが切れた 401 (`unauthorized`) だけ。パスワードやリカバリコードの
 * 誤りも 401 で返るが、それは呼び出し側がエラーとして見せる。
 * 画面の読み込み中 (`navigating`) は何もしない。その 401 は load 側
 * (`fetchOrError` の `loginRedirectFrom`) が戻り先付きで処理する。
 */
client.use({
	async onResponse({ response, schemaPath }) {
		if (response.status !== 401 || ANONYMOUS_PATHS.includes(schemaPath)) return;
		const body: unknown = await response
			.clone()
			.json()
			.catch(() => undefined);
		if (errorCode(body) !== 'unauthorized') return;
		if (navigating.to !== null) return;
		const target = window.location.pathname + window.location.search;
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- withQuery() の戻り値で静的に追えない (→ AGENTS.md「コードの規約」)
		goto(loginPathWithRedirect(resolve('/login'), target));
	}
});
