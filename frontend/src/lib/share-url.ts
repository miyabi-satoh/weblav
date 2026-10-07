/** 同じ PC の中だけで通じるホスト名。ほかの端末に渡すときは、PC の名前に置き換える。 */
function isLoopbackHost(hostname: string): boolean {
	const host = hostname.toLowerCase();
	return (
		host === 'localhost' ||
		host.endsWith('.localhost') ||
		host === '[::1]' ||
		/^127\.\d+\.\d+\.\d+$/.test(host)
	);
}

/**
 * ほかの端末に渡す、開いているページのアドレス (→ docs/ui.md「ページの共有」)。
 * ほかの端末から開いているなら、そのアドレスはもう届くのでそのまま使う (HTTPS の前段を置いた構成も含む)。
 * PC の前で `localhost` から開いているときだけ、PC の名前 (mDNS) とサーバーのポートに置き換える。
 * 置き換えられなければ `undefined`。
 */
export function shareableUrl(
	current: URL,
	connection: { mdnsHostname?: string | null; port: number } | undefined
): string | undefined {
	const path = `${current.pathname}${current.search}`;
	if (!isLoopbackHost(current.hostname)) return `${current.origin}${path}`;
	if (!connection?.mdnsHostname || connection.port === 0) return undefined;
	return `${current.protocol}//${connection.mdnsHostname}:${connection.port}${path}`;
}
