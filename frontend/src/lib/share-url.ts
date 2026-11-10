import type { components } from '$lib/api/schema';

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

/** ほかの端末から開くための、サーバーの名前とアドレス (`GET /api/v1/connection-info`)。 */
export type Connection = components['schemas']['ConnectionInfoResponse'];

/**
 * ほかの端末に渡せる、開いているページのアドレスの候補 (→ docs/ui.md「ページの共有」)。先頭が既定。
 * ほかの端末から開いているなら、そのアドレスはもう届くので先頭に置く (HTTPS の前段を置いた構成も含む)。
 * サーバーへ HTTP で直に届く開き方なら、PC の名前 (mDNS) と LAN の IP アドレスも、サーバーのポートで足す。
 * 名前を引けない端末へは IP で渡すため。HTTPS の前段越しに開いているときは、サーバーのポートへの直の
 * アドレスが前段と同じに届くとは限らないので足さない。
 */
export function shareableUrls(current: URL, connection: Connection | undefined): string[] {
	const path = `${current.pathname}${current.search}`;
	const loopback = isLoopbackHost(current.hostname);
	const urls = loopback ? [] : [`${current.origin}${path}`];
	if (!connection || connection.port === 0 || (!loopback && current.protocol !== 'http:')) {
		return urls;
	}
	const hosts = [connection.mdnsHostname, ...connection.lanAddresses];
	for (const host of hosts) {
		if (!host) continue;
		const url = `${current.protocol}//${host}:${connection.port}${path}`;
		if (!urls.includes(url)) urls.push(url);
	}
	return urls;
}

/**
 * 候補のうち、前に選んだホストのもの。無くなっていれば既定 (先頭)。候補が無ければ `undefined`。
 */
export function pickShareUrl(urls: string[], storedHost: string | null): string | undefined {
	return urls.find((url) => new URL(url).host === storedHost) ?? urls[0];
}
