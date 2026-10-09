/**
 * いま開いている画面が、サーバーになっているパソコン自身のものか。
 *
 * サーバー側は「接続元と `Host` の両方がループバック」で通す (→ docs/access.md「初回セットアップ」・docs/folders.md「公開できるフォルダー」)。
 * ここで見るのはそのうちの `Host` にあたる部分で、**画面に入り口を出すかどうかの判断にだけ使う**。
 * 実際の可否はサーバーが決める (通らなければ 404 になる)。
 *
 * 名前は `localhost` だけを通す。ほかの名前は 127.0.0.1 に向いていても、
 * DNS リバインディングと区別が付かない (サーバー側の判定と揃える)。
 */
export function isLoopbackHost(hostname: string): boolean {
	if (hostname === 'localhost') return true;
	// IPv6 は `[::1]` の形で来るので角括弧を外す。
	const host =
		hostname.startsWith('[') && hostname.endsWith(']') ? hostname.slice(1, -1) : hostname;
	if (host === '::1' || host === '::ffff:127.0.0.1') return true;
	// ループバックは 127.0.0.0/8 全体。
	return /^127\.\d{1,3}\.\d{1,3}\.\d{1,3}$/.test(host);
}
