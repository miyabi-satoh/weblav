// e2e の準備と後片付けを API で行う。
// 確かめたいのは閲覧・管理の画面の動きなので、準備まで画面で組むと遅く、壊れる箇所も増える。
// `request` はブラウザーのコンテキストのもの (`page.request`・`context.request`) を渡し、
// そのコンテキストのログイン状態で呼ぶ。
import type { APIRequestContext } from '@playwright/test';

export type Visibility = 'public' | 'authenticated' | 'private' | 'hidden';

type ContentByApiBody = {
	type: 'link' | 'group' | 'folder' | 'archive';
	title: string;
	url?: string;
	path?: string;
	visibility?: Visibility;
	parentId?: number;
	extensions?: string;
};

type UploadFileByApiBody = {
	title: string;
	fileName: string;
	mimeType: string;
	buffer: Buffer;
};

type ContentByApi = Record<string, unknown> & { id: number };

type RequestBody = Pick<
	NonNullable<Parameters<APIRequestContext['post']>[1]>,
	'data' | 'multipart'
>;

/** `send` の本体。JSON ではなく multipart で送るときはこちらを直接呼ぶ。 */
async function sendBody(
	request: APIRequestContext,
	method: 'get' | 'post' | 'put' | 'delete',
	apiPath: string,
	body: RequestBody
): Promise<unknown> {
	const res = await request[method](`/api/v1${apiPath}`, body);
	if (!res.ok()) {
		throw new Error(
			`${method.toUpperCase()} ${apiPath} が失敗しました (${res.status()}): ${await res.text()}`
		);
	}
	return res.status() === 204 ? null : res.json();
}

/** API を呼び、失敗したら例外にする。204 なら `null` を返す。 */
export async function send(
	request: APIRequestContext,
	method: 'get' | 'post' | 'put' | 'delete',
	apiPath: string,
	data?: unknown
): Promise<unknown> {
	return sendBody(request, method, apiPath, { data });
}

/**
 * 後片付け専用。テスト本体の失敗を覆い隠さず、続く後片付けも止めないよう、
 * ここでの失敗 (例外を含む) は警告に留める。
 */
async function removeByApi(request: APIRequestContext, apiPath: string, label: string) {
	try {
		const res = await request.delete(`/api/v1${apiPath}`);
		if (!res.ok() && res.status() !== 404) {
			console.warn(`${label}の後片付けに失敗しました (${apiPath}): ${res.status()}`);
		}
	} catch (err) {
		console.warn(`${label}の後片付けに失敗しました (${apiPath}):`, err);
	}
}

/** リンクまたはグループを作り、作成時の応答を返す (`PUT` で送り直すときに使う)。 */
async function createContentByApi(
	request: APIRequestContext,
	body: ContentByApiBody
): Promise<ContentByApi> {
	return (await send(request, 'post', '/contents', {
		url: body.type === 'link' ? 'https://example.com/e2e' : undefined,
		...body
	})) as ContentByApi;
}

/** ファイルをアップロードしてコンテンツを作り、id を返す。 */
async function uploadFileByApi(
	request: APIRequestContext,
	body: UploadFileByApiBody
): Promise<number> {
	const content = (await sendBody(request, 'post', '/contents/upload', {
		multipart: {
			title: body.title,
			file: { name: body.fileName, mimeType: body.mimeType, buffer: body.buffer }
		}
	})) as { id: number };
	return content.id;
}

/** 後片付け専用 (→ `removeByApi`)。 */
export async function removeContentByApi(request: APIRequestContext, id: number): Promise<void> {
	await removeByApi(request, `/contents/${id}`, 'コンテンツ');
}

/** API で作ったコンテンツを、テスト本体の成否にかかわらず削除する。 */
export async function withContentByApi<T>(
	request: APIRequestContext,
	body: ContentByApiBody | UploadFileByApiBody,
	fn: (content: ContentByApi) => Promise<T>
): Promise<T> {
	const content =
		'type' in body
			? await createContentByApi(request, body)
			: { id: await uploadFileByApi(request, body) };
	try {
		return await fn(content);
	} finally {
		await removeContentByApi(request, content.id);
	}
}

/**
 * 「公開できるフォルダー」を登録し、id を返す。サーバーからパスが見えないとき (422) は `null`。
 * 呼べるのは admin のログイン状態で、サーバーと同じ PC からだけ。
 */
export async function createRootByApi(
	request: APIRequestContext,
	path: string
): Promise<number | null> {
	const res = await request.post('/api/v1/admin/roots', { data: { path } });
	if (res.status() === 422) return null;
	if (res.status() !== 201) {
		throw new Error(
			`公開できるフォルダーの登録に失敗しました (${res.status()}): ${await res.text()}`
		);
	}
	return ((await res.json()) as { id: number }).id;
}

/** 後片付け専用 (→ `removeByApi`)。 */
export async function removeRootByApi(request: APIRequestContext, id: number): Promise<void> {
	await removeByApi(request, `/admin/roots/${id}`, '公開できるフォルダー');
}

/** 編集者 (`user`) を作り、id を返す。呼べるのは admin のログイン状態だけ。 */
async function createUserByApi(
	request: APIRequestContext,
	username: string,
	password: string
): Promise<number> {
	const user = (await send(request, 'post', '/admin/users', {
		username,
		password,
		role: 'user'
	})) as { id: number };
	return user.id;
}

/** 後片付け専用 (→ `removeByApi`)。 */
async function removeUserByApi(request: APIRequestContext, id: number): Promise<void> {
	await removeByApi(request, `/admin/users/${id}`, 'ユーザー');
}

/** API で作ったユーザーを、テスト本体の成否にかかわらず削除する。 */
export async function withUserByApi<T>(
	request: APIRequestContext,
	username: string,
	password: string,
	fn: (id: number) => Promise<T>
): Promise<T> {
	const id = await createUserByApi(request, username, password);
	try {
		return await fn(id);
	} finally {
		await removeUserByApi(request, id);
	}
}

/** そのコンテキストでログインする。以降のページ遷移もログイン済みになる。 */
export async function logInByApi(
	request: APIRequestContext,
	username: string,
	password: string
): Promise<void> {
	await send(request, 'post', '/auth/login', { username, password });
}
