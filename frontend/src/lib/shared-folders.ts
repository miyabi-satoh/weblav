import { client } from '$lib/api/client';

/**
 * 「共有」が1件でもあるか。
 *
 * 1件も無いとショートカットは登録できないので、種別を選ぶ時点で押せなくする
 * (→ docs/folders.md「共有」)。0件かどうかは、登録先を選ぶ一覧の上位 (= 登録済みのフォルダー)
 * が空かで分かる。
 *
 * 取れなくても画面ごと落とさない。押せるかどうかのヒントでしかなく、実際の可否は
 * 登録の要求でサーバーが決める。
 */
export async function fetchSharedFolderPresence(): Promise<boolean> {
	try {
		const { data } = await client.GET('/api/v1/admin/fs/dirs', {
			params: { query: { path: '' } }
		});
		return (data?.entries.length ?? 0) > 0;
	} catch {
		return false;
	}
}
