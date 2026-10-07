/**
 * 動画サイトのページの URL を、そのサイトの埋め込みプレイヤーの URL にする (→ docs/ui.md「動画サイトの埋め込み」)。
 * 対応していないサイト・動画を指さない URL は `undefined` (リンクのカードのまま新しいタブで開く)。
 */
export function videoEmbedUrl(url: string): string | undefined {
	let parsed: URL;
	try {
		parsed = new URL(url);
	} catch {
		return undefined;
	}
	if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return undefined;
	return youtubeEmbedUrl(parsed);
}

const YOUTUBE_HOSTS = new Set([
	'youtube.com',
	'www.youtube.com',
	'm.youtube.com',
	'youtube-nocookie.com',
	'www.youtube-nocookie.com'
]);

/** YouTube の動画の ID は 11 文字の英数字・`-`・`_`。 */
const YOUTUBE_ID = /^[\w-]{11}$/;

/** パスで動画を指す形 (`/shorts/ID` など)。 */
const YOUTUBE_ID_PATHS = new Set(['shorts', 'embed', 'live', 'v']);

function youtubeEmbedUrl(url: URL): string | undefined {
	const host = url.hostname.toLowerCase();
	const segments = url.pathname.split('/').filter(Boolean);
	let id: string | undefined;
	if (host === 'youtu.be') {
		id = segments[0];
	} else if (YOUTUBE_HOSTS.has(host)) {
		if (segments.length === 1 && segments[0] === 'watch')
			id = url.searchParams.get('v') ?? undefined;
		else if (segments.length >= 2 && YOUTUBE_ID_PATHS.has(segments[0])) id = segments[1];
	}
	if (!id || !YOUTUBE_ID.test(id)) return undefined;

	// ADR: Cookie を置かない youtube-nocookie.com の口を使う。閲覧するのは生徒の端末で、再生するまで
	// YouTube の Cookie を置かせないため。
	const embed = new URL(`https://www.youtube-nocookie.com/embed/${id}`);
	const start = youtubeStartSeconds(url.searchParams.get('t') ?? url.searchParams.get('start'));
	if (start) embed.searchParams.set('start', String(start));
	return embed.href;
}

/** `t` の値 (`90`・`90s`・`1m30s`・`1h2m3s`) を秒にする。読めなければ `undefined`。 */
function youtubeStartSeconds(value: string | null): number | undefined {
	if (!value) return undefined;
	const match = /^(?:(\d+)h)?(?:(\d+)m)?(?:(\d+)s?)?$/.exec(value);
	if (!match) return undefined;
	const [, hours = '0', minutes = '0', seconds = '0'] = match;
	const total = Number(hours) * 3600 + Number(minutes) * 60 + Number(seconds);
	return total > 0 ? total : undefined;
}
