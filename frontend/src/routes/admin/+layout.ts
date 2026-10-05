import type { LayoutLoad } from './$types';

// 未ログインは親(ルート `+layout.ts`)が `/admin` 配下だけを対象に `/login` へ
// リダイレクトする。ここでの role チェックは行わない: コンテンツ管理は `user` にも
// 開放されている(→ docs/access.md「ロールと操作」・「管理画面の一覧が `user` に見えること」)。
//
// admin 限定の画面(ユーザー管理・システム設定など)を足すときは、この layout ではなく
// その画面ごとにガードを置くこと。ここで一律に弾くと `user` がコンテンツ管理に
// たどり着けなくなる。
export const load: LayoutLoad = async ({ parent }) => {
	await parent();
	return {};
};
