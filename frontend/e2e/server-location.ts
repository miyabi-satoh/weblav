// e2e を当てているサーバーが、このマシンで動いているか。
//
// 公開できるフォルダー・セットアップの口はサーバーの PC からしか通らない (→ docs/access.md「初回セットアップ」・docs/folders.md「公開できるフォルダー」)。
// 別のマシン (別の PC のリリース版など) に当てるときは、それらと、フィクスチャの木を
// 登録して使うテストを飛ばし、残りを流す。
import { isLoopbackHost } from '../src/lib/loopback';

export const SERVER_IS_REMOTE = !isLoopbackHost(new URL(process.env.E2E_BASE_URL ?? '').hostname);

export const SERVER_IS_REMOTE_REASON =
	'サーバーが別のマシンで動いている (サーバーの PC からしか使えない口とフィクスチャに頼る)';
